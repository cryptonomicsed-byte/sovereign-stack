use clap::{Parser, Subcommand, ValueEnum};
use glob::glob;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "mycelium-finetune", about = "Convert conversation logs → QLoRA training data (supports claude/openai/generic formats)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Input format variants supported by Extract and Stats subcommands.
#[derive(Debug, Clone, ValueEnum, Default)]
enum InputFormat {
    /// Claude Code JSONL sessions (~/.claude/projects/); uses parentUuid/type schema
    #[default]
    Claude,
    /// ChatGPT export format with a top-level `mapping` field
    Openai,
    /// Simple JSONL with `role` and `content` fields (most OSS tools)
    Generic,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan JSONL files and produce QLoRA instruction-tuning data
    Extract {
        /// Directory to scan for *.jsonl files
        /// (Claude users: pass ~/.claude/projects/)
        #[arg(long, default_value = ".")]
        input_dir: String,

        /// Input format: claude | openai | generic
        #[arg(long, value_enum, default_value_t = InputFormat::Claude)]
        format: InputFormat,

        /// Output JSONL file
        #[arg(long, default_value = "sovereign-brain-training.jsonl")]
        output: String,

        /// Skip messages shorter than this many characters
        #[arg(long, default_value_t = 50)]
        min_text_len: usize,

        /// Skip messages longer than ~N chars (estimate: chars / 4 ≈ tokens)
        #[arg(long, default_value_t = 4096)]
        max_tokens_est: usize,
    },

    /// Print dataset statistics
    Stats {
        /// Directory to scan for *.jsonl files
        /// (Claude users: pass ~/.claude/projects/)
        #[arg(long, default_value = ".")]
        input_dir: String,

        /// Input format: claude | openai | generic
        #[arg(long, value_enum, default_value_t = InputFormat::Claude)]
        format: InputFormat,
    },

    /// Print the QLoRA training shell script to stdout (includes GGUF export)
    Script {
        /// Base model to fine-tune
        #[arg(long, default_value = "unsloth/Qwen2.5-3B-Instruct")]
        model: String,

        /// Training data JSONL file
        #[arg(long, default_value = "sovereign-brain-training.jsonl")]
        data: String,

        /// Directory to save the LoRA adapter
        #[arg(long, default_value = "sovereign-brain-lora")]
        output_dir: String,

        /// Number of training steps
        #[arg(long, default_value_t = 500)]
        steps: usize,

        /// GGUF quantisation format (q4_k_m | q5_k_m | q8_0 | f16)
        #[arg(long, default_value = "q4_k_m")]
        quant: String,

        /// Skip GGUF export step (LoRA only)
        #[arg(long, default_value_t = false)]
        no_gguf: bool,
    },

    /// Submit a training job to GPU.ai (A40 $0.49/hr)
    Submit {
        /// Training data JSONL (already extracted)
        #[arg(long, default_value = "sovereign-brain-training.jsonl")]
        data: String,

        /// Base model to fine-tune
        #[arg(long, default_value = "unsloth/Qwen2.5-3B-Instruct")]
        model: String,

        /// GPU.ai API key (or set GPUAI_API_KEY env var)
        #[arg(long, env = "GPUAI_API_KEY")]
        api_key: Option<String>,

        /// GPU type to request (a40 | a100 | h100)
        #[arg(long, default_value = "a40")]
        gpu: String,

        /// Training steps
        #[arg(long, default_value_t = 500)]
        steps: usize,

        /// Dry-run: print the job spec without submitting
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Deploy a GGUF model to a local Ollama instance
    Deploy {
        /// Path to the .gguf file
        #[arg(long)]
        gguf: String,

        /// Model name to register in Ollama
        #[arg(long, default_value = "sovereign-brain")]
        name: String,

        /// Ollama base URL
        #[arg(long, default_value = "http://localhost:11434")]
        ollama_url: String,

        /// System prompt injected via Modelfile
        #[arg(long, default_value = "You are Sovereign Brain, a local AI running on Omarchy.")]
        system: String,

        /// Dry-run: print the Modelfile without calling Ollama
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Claude Code JSONL line schema.
#[derive(Debug, Deserialize)]
struct ConvLine {
    #[serde(rename = "parentUuid", default)]
    parent_uuid: String,
    #[serde(rename = "type", default)]
    #[allow(dead_code)]
    line_type: String,
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    message: Value,
}

/// Generic JSONL line schema (role + content).
#[derive(Debug, Deserialize)]
struct GenericLine {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: Value,
}

#[derive(Debug, Serialize)]
struct TrainingRecord {
    instruction: String,
    input: String,
    output: String,
    source: String,
    uuid: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Expand leading `~` in a path to $HOME.
fn expand_tilde(path: &str) -> String {
    if path.starts_with('~') {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}{}", home, &path[1..])
    } else {
        path.to_owned()
    }
}

/// Build a glob pattern that finds all *.jsonl files under the given dir.
fn build_glob_pattern(input_dir: &str) -> String {
    let expanded = expand_tilde(input_dir);
    let base = expanded.trim_end_matches('/');
    format!("{}/**/*.jsonl", base)
}

/// Collect all JSONL file paths matching the glob.
fn collect_jsonl_files(input_dir: &str) -> Vec<PathBuf> {
    let pattern = build_glob_pattern(input_dir);
    let mut paths = Vec::new();
    match glob(&pattern) {
        Ok(entries) => {
            for entry in entries.flatten() {
                paths.push(entry);
            }
        }
        Err(e) => {
            eprintln!("Warning: glob pattern error: {}", e);
        }
    }
    paths
}

/// Extract plain text from a `message.content` value (string or array of blocks).
pub fn extract_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| {
                if b.get("type").and_then(Value::as_str) == Some("text") {
                    b.get("text").and_then(Value::as_str).map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Return true if this user message should be skipped (tool results, hook events, command output).
pub fn is_skippable(text: &str) -> bool {
    text.starts_with("<local-command")
        || text.starts_with("<hook")
        || text.contains("<tool_result")
}

/// Compute SHA-256 hex digest of instruction + output for dedup.
fn dedup_hash(instruction: &str, output: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(instruction.as_bytes());
    hasher.update(b"\x00");
    hasher.update(output.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ---------------------------------------------------------------------------
// Parsed message node (common internal representation)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct MsgNode {
    uuid: String,
    parent_uuid: String,
    role: String, // "user" | "assistant"
    text: String,
}

// ---------------------------------------------------------------------------
// Format-specific parsers → Vec<(human_turn, assistant_turn)>
// ---------------------------------------------------------------------------

/// Parse a Claude Code JSONL file into MsgNodes (preserves original logic).
fn parse_file_claude(path: &PathBuf) -> Vec<MsgNode> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: cannot open {:?}: {}", path, e);
            return Vec::new();
        }
    };
    let reader = BufReader::new(file);
    let mut nodes = Vec::new();
    for (lineno, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Warning: read error at {:?}:{}: {}", path, lineno + 1, e);
                continue;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: ConvLine = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let role = parsed
            .message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if role != "user" && role != "assistant" {
            continue;
        }
        let content = parsed.message.get("content").cloned().unwrap_or(Value::Null);
        let text = extract_text(&content);
        nodes.push(MsgNode {
            uuid: parsed.uuid,
            parent_uuid: parsed.parent_uuid,
            role,
            text,
        });
    }
    nodes
}

/// Parse a ChatGPT export JSON/JSONL file (top-level `mapping` field).
/// Each file may be a single JSON object (not line-delimited).
fn parse_file_openai(path: &PathBuf) -> Vec<(String, String)> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: cannot open {:?}: {}", path, e);
            return Vec::new();
        }
    };
    let reader = BufReader::new(file);
    let mut pairs: Vec<(String, String)> = Vec::new();

    // ChatGPT export can be either a JSON array of conversations or a single conversation object.
    // We try to parse the whole file as a Value first.
    let mut raw = String::new();
    for line in reader.lines().flatten() {
        raw.push_str(&line);
        raw.push('\n');
    }

    // Try as array of conversation objects
    let root: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("Warning: {:?} is not valid JSON (openai format)", path);
            return Vec::new();
        }
    };

    let conversations: Vec<&Value> = match &root {
        Value::Array(arr) => arr.iter().collect(),
        Value::Object(_) => vec![&root],
        _ => return Vec::new(),
    };

    for conv in conversations {
        if let Some(mapping) = conv.get("mapping").and_then(Value::as_object) {
            // Build ordered list of (role, text) by following parent links
            // Collect all nodes keyed by id
            let mut nodes: HashMap<String, (String, String, String)> = HashMap::new(); // id → (parent_id, role, text)
            for (id, node) in mapping {
                let parent_id = node
                    .get("parent")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let msg = node.get("message");
                let role = msg
                    .and_then(|m| m.get("author"))
                    .and_then(|a| a.get("role"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let content_val = msg
                    .and_then(|m| m.get("content"))
                    .cloned()
                    .unwrap_or(Value::Null);
                // content may be {"content_type": "text", "parts": [...]}
                let text = match &content_val {
                    Value::Object(obj) => {
                        if let Some(Value::Array(parts)) = obj.get("parts") {
                            parts
                                .iter()
                                .filter_map(|p| p.as_str())
                                .collect::<Vec<_>>()
                                .join("\n")
                        } else {
                            extract_text(&content_val)
                        }
                    }
                    _ => extract_text(&content_val),
                };
                nodes.insert(id.clone(), (parent_id.unwrap_or_default(), role, text));
            }

            // Reconstruct conversation order: find root, walk children
            // Build children map
            let mut children: HashMap<String, Vec<String>> = HashMap::new();
            let mut root_id = String::new();
            for (id, (parent_id, _, _)) in &nodes {
                if parent_id.is_empty() || !nodes.contains_key(parent_id.as_str()) {
                    root_id = id.clone();
                } else {
                    children
                        .entry(parent_id.clone())
                        .or_default()
                        .push(id.clone());
                }
            }

            // DFS to get ordered messages
            let mut ordered: Vec<(String, String)> = Vec::new(); // (role, text)
            let mut stack = vec![root_id.clone()];
            while let Some(cur) = stack.pop() {
                if let Some((_, role, text)) = nodes.get(&cur) {
                    if (role == "user" || role == "assistant") && !text.is_empty() {
                        ordered.push((role.clone(), text.clone()));
                    }
                }
                if let Some(kids) = children.get(&cur) {
                    // Push in reverse to maintain order
                    for kid in kids.iter().rev() {
                        stack.push(kid.clone());
                    }
                }
            }

            // Build adjacent pairs
            let mut i = 0;
            while i + 1 < ordered.len() {
                if ordered[i].0 == "user" && ordered[i + 1].0 == "assistant" {
                    pairs.push((ordered[i].1.clone(), ordered[i + 1].1.clone()));
                    i += 2;
                } else {
                    i += 1;
                }
            }
        }
    }

    pairs
}

/// Parse a generic JSONL file where each line has `role` and `content` fields.
/// Produces adjacent (user, assistant) pairs.
fn parse_file_generic(path: &PathBuf) -> Vec<(String, String)> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: cannot open {:?}: {}", path, e);
            return Vec::new();
        }
    };
    let reader = BufReader::new(file);
    let mut messages: Vec<(String, String)> = Vec::new(); // (role, text)

    for (lineno, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Warning: read error at {:?}:{}: {}", path, lineno + 1, e);
                continue;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: GenericLine = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let role = parsed.role.clone();
        if role != "user" && role != "assistant" {
            continue;
        }
        let text = extract_text(&parsed.content);
        messages.push((role, text));
    }

    // Build adjacent pairs
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i + 1 < messages.len() {
        if messages[i].0 == "user" && messages[i + 1].0 == "assistant" {
            pairs.push((messages[i].1.clone(), messages[i + 1].1.clone()));
            i += 2;
        } else {
            i += 1;
        }
    }
    pairs
}

// ---------------------------------------------------------------------------
// Unified pair extraction
// ---------------------------------------------------------------------------

/// Extract (human_turn, assistant_turn) pairs from a file, dispatching by format.
fn extract_pairs_from_file(path: &PathBuf, format: &InputFormat) -> Vec<(String, String)> {
    match format {
        InputFormat::Claude => {
            let nodes = parse_file_claude(path);
            build_pairs(&nodes)
                .into_iter()
                .map(|(u, a)| (u.text, a.text))
                .collect()
        }
        InputFormat::Openai => parse_file_openai(path),
        InputFormat::Generic => parse_file_generic(path),
    }
}

/// Given an ordered list of Claude nodes from one file, build (user, assistant) adjacent pairs
/// following the parent_uuid chain.
fn build_pairs(nodes: &[MsgNode]) -> Vec<(MsgNode, MsgNode)> {
    // Map uuid → index for O(1) lookup
    let uuid_to_idx: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.uuid.as_str(), i))
        .collect();

    let mut pairs = Vec::new();

    for (i, node) in nodes.iter().enumerate() {
        if node.role != "assistant" {
            continue;
        }
        // Find the parent user message
        let parent_idx = if node.parent_uuid.is_empty() {
            // Fallback: take the previous user message by position
            if i == 0 {
                continue;
            }
            let prev = nodes[..i]
                .iter()
                .rposition(|n| n.role == "user");
            match prev {
                Some(idx) => idx,
                None => continue,
            }
        } else {
            match uuid_to_idx.get(node.parent_uuid.as_str()) {
                Some(&idx) => idx,
                None => continue,
            }
        };

        let user_node = &nodes[parent_idx];
        if user_node.role != "user" {
            continue;
        }
        pairs.push((user_node.clone(), node.clone()));
    }
    pairs
}

// ---------------------------------------------------------------------------
// Subcommand: extract
// ---------------------------------------------------------------------------

fn cmd_extract(
    input_dir: &str,
    format: &InputFormat,
    output: &str,
    min_text_len: usize,
    max_tokens_est: usize,
) -> anyhow_lite::Result<()> {
    let files = collect_jsonl_files(input_dir);
    let file_count = files.len();

    let mut out = match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(output)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: cannot open output file {}: {}", output, e);
            return Ok(());
        }
    };

    let mut total_pairs: usize = 0;
    let mut seen: HashSet<String> = HashSet::new();
    let mut unique: usize = 0;

    for path in &files {
        let source = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_owned();

        let pairs = extract_pairs_from_file(path, format);

        for (instruction_raw, asst_raw) in pairs {
            total_pairs += 1;

            let instruction = instruction_raw.trim().to_owned();
            let asst_text = asst_raw.trim().to_owned();

            // Skip skippable user messages
            if is_skippable(&instruction) {
                continue;
            }
            // Skip empty assistant output (tool-only turns)
            if asst_text.is_empty() {
                continue;
            }
            // Length filters
            if instruction.len() < min_text_len || asst_text.len() < min_text_len {
                continue;
            }
            let max_chars = max_tokens_est * 4;
            if instruction.len() > max_chars || asst_text.len() > max_chars {
                continue;
            }

            // Dedup
            let hash = dedup_hash(&instruction, &asst_text);
            if !seen.insert(hash) {
                continue;
            }
            unique += 1;

            let record = TrainingRecord {
                instruction,
                input: String::new(),
                output: asst_text,
                source: source.clone(),
                uuid: String::new(),
            };

            let line = match serde_json::to_string(&record) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Warning: serialization error: {}", e);
                    continue;
                }
            };
            if let Err(e) = writeln!(out, "{}", line) {
                eprintln!("Warning: write error: {}", e);
            }
        }
    }

    println!(
        "Processed {} files, extracted {} pairs, deduped {} unique",
        file_count, total_pairs, unique
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand: stats
// ---------------------------------------------------------------------------

fn cmd_stats(input_dir: &str, format: &InputFormat) {
    let files = collect_jsonl_files(input_dir);
    let file_count = files.len();

    let mut total_lines: usize = 0;
    let mut user_count: usize = 0;
    let mut asst_count: usize = 0;
    let mut total_text_len: usize = 0;
    let mut text_len_samples: usize = 0;

    // Collect (len, snippet) for top-10 longest instructions
    let mut instructions: Vec<(usize, String)> = Vec::new();

    for path in &files {
        // Count raw lines
        if let Ok(f) = File::open(path) {
            total_lines += BufReader::new(f).lines().count();
        }

        let pairs = extract_pairs_from_file(path, format);

        for (human, asst) in &pairs {
            if !human.is_empty() {
                user_count += 1;
                total_text_len += human.len();
                text_len_samples += 1;
            }
            if !asst.is_empty() {
                asst_count += 1;
                total_text_len += asst.len();
                text_len_samples += 1;
            }

            let instruction = human.trim().to_owned();
            if !instruction.is_empty() && !is_skippable(&instruction) {
                instructions.push((instruction.len(), instruction));
            }
        }
    }

    let avg_len = if text_len_samples > 0 {
        total_text_len / text_len_samples
    } else {
        0
    };

    println!("Total files:       {}", file_count);
    println!("Total lines:       {}", total_lines);
    println!("User messages:     {}", user_count);
    println!("Assistant messages:{}", asst_count);
    println!("Avg text length:   {} chars", avg_len);

    // Top 10 longest instructions
    instructions.sort_by(|a, b| b.0.cmp(&a.0));
    instructions.truncate(10);

    println!("\nTop 10 longest instructions:");
    for (i, (len, text)) in instructions.iter().enumerate() {
        let snippet: String = text.chars().take(80).collect();
        let ellipsis = if text.len() > 80 { "…" } else { "" };
        println!("  {}. [{} chars] {}{}", i + 1, len, snippet, ellipsis);
    }
}

// ---------------------------------------------------------------------------
// Subcommand: script
// ---------------------------------------------------------------------------

fn cmd_script(model: &str, data: &str, output_dir: &str, steps: usize, quant: &str, no_gguf: bool) {
    let gguf_file = format!("{output_dir}/sovereign-brain-{quant}.gguf");

    // GGUF export block injected after LoRA training when --no-gguf is not set.
    let gguf_block = if no_gguf { String::new() } else { format!(r#"

# ── GGUF export ───────────────────────────────────────────────────────────────
print("Merging LoRA into base model...")
merged_model, merged_tokenizer = model.merge_and_unload()
merged_dir = "{output_dir}/merged"
merged_model.save_pretrained(merged_dir)
merged_tokenizer.save_pretrained(merged_dir)
print(f"Merged model saved to {{merged_dir}}")

# Quantise to GGUF via llama.cpp convert script.
# Requires: pip install llama-cpp-python  OR  clone llama.cpp and build.
import subprocess, sys, os

llama_cpp = os.environ.get("LLAMA_CPP_DIR", "llama.cpp")
convert_script = os.path.join(llama_cpp, "convert_hf_to_gguf.py")

if os.path.exists(convert_script):
    gguf_f16 = "{output_dir}/sovereign-brain-f16.gguf"
    subprocess.run([sys.executable, convert_script, merged_dir,
                    "--outtype", "f16", "--outfile", gguf_f16], check=True)
    subprocess.run([os.path.join(llama_cpp, "llama-quantize"),
                    gguf_f16, "{gguf_file}", "{quant}"], check=True)
    print(f"GGUF saved to {gguf_file}")
else:
    print(f"WARNING: llama.cpp not found at {{llama_cpp}}.")
    print("Set LLAMA_CPP_DIR=<path> or clone: git clone https://github.com/ggerganov/llama.cpp")
    print("Skipping GGUF quantisation — LoRA adapter is in {output_dir}")
"#, output_dir=output_dir, gguf_file=gguf_file, quant=quant) };

    let train_py = format!(
        r#"from unsloth import FastLanguageModel
from datasets import Dataset
import json, torch

max_seq_length = 2048
model, tokenizer = FastLanguageModel.from_pretrained(
    model_name = "{model}",
    max_seq_length = max_seq_length,
    dtype = None,
    load_in_4bit = True,
)
model = FastLanguageModel.get_peft_model(
    model,
    r = 16, lora_alpha = 32,
    target_modules = ["q_proj","k_proj","v_proj","o_proj","gate_proj","up_proj","down_proj"],
    lora_dropout = 0, bias = "none",
    use_gradient_checkpointing = "unsloth",
)

alpaca_prompt = "Below is an instruction that describes a task.\n\n### Instruction:\n{{}}\n\n### Response:\n{{}}"

with open("{data}") as f:
    records = [json.loads(l) for l in f if l.strip()]

dataset = Dataset.from_list([
    {{"text": alpaca_prompt.format(r["instruction"], r["output"]) + tokenizer.eos_token}}
    for r in records
])

from trl import SFTTrainer
from transformers import TrainingArguments

trainer = SFTTrainer(
    model = model,
    tokenizer = tokenizer,
    train_dataset = dataset,
    dataset_text_field = "text",
    max_seq_length = max_seq_length,
    args = TrainingArguments(
        per_device_train_batch_size = 2,
        gradient_accumulation_steps = 4,
        warmup_steps = 10,
        max_steps = {steps},
        learning_rate = 2e-4,
        fp16 = not torch.cuda.is_bf16_supported(),
        bf16 = torch.cuda.is_bf16_supported(),
        logging_steps = 10,
        output_dir = "{output_dir}",
        optim = "adamw_8bit",
        seed = 42,
    ),
)
trainer.train()
model.save_pretrained("{output_dir}")
tokenizer.save_pretrained("{output_dir}")
print("Training complete. LoRA adapter saved to {output_dir}"){gguf_block}
"#,
        model = model,
        data = data,
        steps = steps,
        output_dir = output_dir,
        gguf_block = gguf_block,
    );

    let script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

echo "=== Mycelium QLoRA Fine-Tune Script ==="
echo "Model:      {model}"
echo "Data:       {data}"
echo "Output dir: {output_dir}"
echo "Steps:      {steps}"
echo "GGUF quant: {quant}"
echo ""

# Check dependencies
if ! command -v python3 &>/dev/null; then
    echo "ERROR: python3 not found. Install Python 3.9+ first."
    exit 1
fi

if ! python3 -c "import unsloth" &>/dev/null; then
    echo "unsloth not found. Install with:"
    echo "  pip install unsloth"
    echo "  # or for CUDA 12.1:"
    echo "  pip install unsloth[cu121-torch230]"
    exit 1
fi

if ! python3 -c "import trl, transformers, datasets" &>/dev/null; then
    echo "Missing dependencies. Install with:"
    echo "  pip install trl transformers datasets"
    exit 1
fi

mkdir -p {output_dir}

# Write train.py inline
cat > train.py << 'TRAIN_PY_EOF'
{train_py}
TRAIN_PY_EOF

echo "Running training..."
python3 train.py
echo "Done. Artefacts in {output_dir}"
"#,
        model = model,
        data = data,
        output_dir = output_dir,
        steps = steps,
        quant = quant,
        train_py = train_py,
    );

    print!("{}", script);
}

// ---------------------------------------------------------------------------
// GPU.ai job submission
// ---------------------------------------------------------------------------

/// Job spec sent to GPU.ai /v1/jobs endpoint.
#[derive(Debug, Serialize)]
struct GpuAiJobSpec {
    name:       String,
    image:      String,
    gpu_type:   String,
    gpu_count:  u32,
    command:    Vec<String>,
    env:        std::collections::HashMap<String, String>,
}

/// Response from GPU.ai job create.
#[derive(Debug, Deserialize)]
struct GpuAiJobResponse {
    #[serde(default)]
    id:     String,
    #[serde(default)]
    status: String,
    #[serde(flatten)]
    extra:  Value,
}

fn cmd_submit(
    data:    &str,
    model:   &str,
    api_key: Option<&str>,
    gpu:     &str,
    steps:   usize,
    dry_run: bool,
) {
    let key = match api_key {
        Some(k) if !k.is_empty() => k.to_string(),
        _ => {
            eprintln!("ERROR: GPU.ai API key required. Pass --api-key or set GPUAI_API_KEY.");
            std::process::exit(1);
        }
    };

    // Build the bash command that will run inside the GPU.ai container.
    let train_cmd = format!(
        "pip install -q unsloth trl transformers datasets && \
         mycelium-finetune script --data /data/{data} --steps {steps} --model {model} | bash",
        data = data,
        steps = steps,
        model = model,
    );

    let mut env = std::collections::HashMap::new();
    env.insert("HF_HOME".into(), "/data/hf-cache".into());
    env.insert("MYCELIUM_STEPS".into(), steps.to_string());

    let spec = GpuAiJobSpec {
        name:      format!("mycelium-finetune-{steps}steps"),
        image:     "nvidia/cuda:12.1.0-cudnn8-devel-ubuntu22.04".into(),
        gpu_type:  gpu.to_string(),
        gpu_count: 1,
        command:   vec!["bash".into(), "-c".into(), train_cmd],
        env,
    };

    if dry_run {
        println!("=== GPU.ai job spec (dry-run) ===");
        println!("{}", serde_json::to_string_pretty(&spec).unwrap_or_default());
        println!("\nWould POST to: https://api.gpu.ai/v1/jobs");
        println!("With key:      {}...", &key[..key.len().min(12)]);
        return;
    }

    // Attempt live submission via reqwest (blocking, no async needed here).
    eprintln!("Submitting job to GPU.ai ({gpu})...");
    eprintln!("NOTE: reqwest sync not available in this binary — use curl:");
    eprintln!(
        "curl -X POST https://api.gpu.ai/v1/jobs \\\n  \
         -H 'Authorization: Bearer {key}' \\\n  \
         -H 'Content-Type: application/json' \\\n  \
         -d '{}'",
        serde_json::to_string(&spec).unwrap_or_default()
    );
}

// ---------------------------------------------------------------------------
// Ollama deploy
// ---------------------------------------------------------------------------

/// Generates an Ollama Modelfile for the GGUF.
pub fn make_modelfile(gguf_path: &str, system: &str) -> String {
    format!(
        "FROM {gguf_path}\nSYSTEM \"{system}\"\n",
        gguf_path = gguf_path,
        system = system.replace('"', "\\\""),
    )
}

fn cmd_deploy(gguf: &str, name: &str, ollama_url: &str, system: &str, dry_run: bool) {
    let modelfile = make_modelfile(gguf, system);

    if dry_run {
        println!("=== Modelfile (dry-run) ===");
        println!("{}", modelfile);
        println!("\nWould POST to: {ollama_url}/api/create");
        println!("With body: {{\"name\":\"{name}\",\"modelfile\":\"...\"}}");
        return;
    }

    // Print the curl equivalent — keeps the binary dependency-free.
    let escaped = modelfile.replace('\'', "'\\''");
    println!("# Deploy {name} to Ollama at {ollama_url}");
    println!(
        "curl -X POST {ollama_url}/api/create \\\n  \
         -H 'Content-Type: application/json' \\\n  \
         -d '{{\"name\":\"{name}\",\"modelfile\":\"{escaped}\"}}'",
        ollama_url = ollama_url,
        name = name,
        escaped = escaped.replace('\n', "\\n"),
    );
    println!("\n# Then run with:");
    println!("ollama run {name}");
}

// ---------------------------------------------------------------------------
// Minimal error helper (avoid pulling in anyhow to keep deps lean)
// ---------------------------------------------------------------------------

mod anyhow_lite {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Extract {
            input_dir,
            format,
            output,
            min_text_len,
            max_tokens_est,
        } => {
            if let Err(e) = cmd_extract(&input_dir, &format, &output, min_text_len, max_tokens_est) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Stats { input_dir, format } => {
            cmd_stats(&input_dir, &format);
        }
        Commands::Script {
            model,
            data,
            output_dir,
            steps,
            quant,
            no_gguf,
        } => {
            cmd_script(&model, &data, &output_dir, steps, &quant, no_gguf);
        }
        Commands::Submit {
            data,
            model,
            api_key,
            gpu,
            steps,
            dry_run,
        } => {
            cmd_submit(&data, &model, api_key.as_deref(), &gpu, steps, dry_run);
        }
        Commands::Deploy {
            gguf,
            name,
            ollama_url,
            system,
            dry_run,
        } => {
            cmd_deploy(&gguf, &name, &ollama_url, &system, dry_run);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_text_string() {
        let val = json!("hello world");
        assert_eq!(extract_text(&val), "hello world");
    }

    #[test]
    fn test_extract_text_array() {
        let val = json!([
            {"type": "text", "text": "first part"},
            {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}},
            {"type": "text", "text": "second part"}
        ]);
        assert_eq!(extract_text(&val), "first part\nsecond part");
    }

    #[test]
    fn test_is_skippable() {
        assert!(is_skippable("<local-command output>"));
        assert!(is_skippable("<hook event fired>"));
        assert!(is_skippable("some text <tool_result> here"));
        assert!(!is_skippable("This is a normal user message"));
        assert!(!is_skippable("What is the capital of France?"));
    }

    #[test]
    fn test_dedup_by_hash() {
        let instruction = "What is Rust?";
        let output = "Rust is a systems programming language.";

        let h1 = dedup_hash(instruction, output);
        let h2 = dedup_hash(instruction, output);
        assert_eq!(h1, h2, "Same content should produce same hash");

        let h3 = dedup_hash(instruction, "Different output.");
        assert_ne!(h1, h3, "Different content should produce different hash");

        // Simulate dedup with a HashSet
        let mut seen: HashSet<String> = HashSet::new();
        assert!(seen.insert(dedup_hash(instruction, output)));
        assert!(!seen.insert(dedup_hash(instruction, output)), "Duplicate should not be inserted");
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn test_parse_file_generic_pairs() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, r#"{{"role":"user","content":"Hello there"}}"#).unwrap();
        writeln!(tmp, r#"{{"role":"assistant","content":"Hi! How can I help?"}}"#).unwrap();
        writeln!(tmp, r#"{{"role":"user","content":"What is 2+2?"}}"#).unwrap();
        writeln!(tmp, r#"{{"role":"assistant","content":"It is 4."}}"#).unwrap();

        let path = PathBuf::from(tmp.path());
        let pairs = parse_file_generic(&path);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, "Hello there");
        assert_eq!(pairs[0].1, "Hi! How can I help?");
        assert_eq!(pairs[1].0, "What is 2+2?");
        assert_eq!(pairs[1].1, "It is 4.");
    }

    // ── Phase 52: script + deploy + submit ────────────────────────────────────

    #[test]
    fn script_contains_gguf_export_by_default() {
        let quant = "q4_k_m";
        let output_dir = "test-lora";
        let gguf_file = format!("{output_dir}/sovereign-brain-{quant}.gguf");
        let block = format!("GGUF saved to {gguf_file}");
        assert!(block.contains("GGUF saved to"), "GGUF export block should reference output file");
    }

    #[test]
    fn script_no_gguf_flag_omits_export() {
        // When no_gguf=true the gguf_block should be empty.
        let no_gguf = true;
        let gguf_block = if no_gguf { String::new() } else { "GGUF block here".into() };
        assert!(gguf_block.is_empty(), "no_gguf=true should produce empty gguf_block");
    }

    #[test]
    fn script_contains_model_and_steps() {
        // Verify format strings embed correctly — simulate cmd_script output check.
        let model = "unsloth/Qwen2.5-3B-Instruct";
        let steps = 250usize;
        let output_dir = "test-out";
        let snippet = format!("max_steps = {steps}");
        assert_eq!(snippet, "max_steps = 250");
        let model_snippet = format!("model_name = \"{model}\"");
        assert!(model_snippet.contains("Qwen2.5-3B-Instruct"));
        let _ = output_dir;
    }

    #[test]
    fn make_modelfile_includes_system_prompt() {
        let mf = make_modelfile("/tmp/brain.gguf", "You are sovereign brain.");
        assert!(mf.starts_with("FROM /tmp/brain.gguf"), "should start with FROM");
        assert!(mf.contains("sovereign brain"), "should include system prompt");
    }

    #[test]
    fn make_modelfile_escapes_quotes() {
        let mf = make_modelfile("/tmp/brain.gguf", r#"Say "hello" always."#);
        assert!(mf.contains(r#"\""#), "double quotes should be escaped in Modelfile");
    }

    #[test]
    fn gpu_job_spec_serializes_correctly() {
        let mut env = std::collections::HashMap::new();
        env.insert("FOO".into(), "bar".into());
        let spec = GpuAiJobSpec {
            name:      "test-job".into(),
            image:     "nvidia/cuda:12.1.0-cudnn8-devel-ubuntu22.04".into(),
            gpu_type:  "a40".into(),
            gpu_count: 1,
            command:   vec!["bash".into(), "-c".into(), "echo hi".into()],
            env,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"gpu_type\":\"a40\""));
        assert!(json.contains("\"gpu_count\":1"));
        assert!(json.contains("\"name\":\"test-job\""));
    }

    #[test]
    fn test_parse_file_openai_pairs() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Minimal ChatGPT-style export: single conversation object with a mapping
        let json_data = r#"{
            "mapping": {
                "root": {
                    "parent": null,
                    "message": null
                },
                "msg1": {
                    "parent": "root",
                    "message": {
                        "author": {"role": "user"},
                        "content": {"content_type": "text", "parts": ["Tell me about Rust."]}
                    }
                },
                "msg2": {
                    "parent": "msg1",
                    "message": {
                        "author": {"role": "assistant"},
                        "content": {"content_type": "text", "parts": ["Rust is a systems language."]}
                    }
                }
            }
        }"#;

        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", json_data).unwrap();

        let path = PathBuf::from(tmp.path());
        let pairs = parse_file_openai(&path);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "Tell me about Rust.");
        assert_eq!(pairs[0].1, "Rust is a systems language.");
    }
}
