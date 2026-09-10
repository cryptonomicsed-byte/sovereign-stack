//! Pre/post-invocation hook chain.
//! Hooks run before and after every tool invocation, enabling:
//!   - Audit logging
//!   - Rate limiting
//!   - Capability injection
//!   - Output filtering

use serde_json::Value;

pub type HookResult = Result<(), String>;
pub type HookFn = Box<dyn Fn(&str, &Value) -> HookResult + Send + Sync>;

#[derive(Default)]
pub struct HookChain {
    pre_hooks:  Vec<HookFn>,
    post_hooks: Vec<HookFn>,
}

impl HookChain {
    pub fn new() -> Self { Self::default() }

    pub fn pre(mut self, f: HookFn) -> Self {
        self.pre_hooks.push(f);
        self
    }

    pub fn post(mut self, f: HookFn) -> Self {
        self.post_hooks.push(f);
        self
    }

    pub fn run_pre(&self, tool: &str, args: &Value) -> HookResult {
        for hook in &self.pre_hooks {
            hook(tool, args)?;
        }
        Ok(())
    }

    pub fn run_post(&self, tool: &str, result: &Value) -> HookResult {
        for hook in &self.post_hooks {
            hook(tool, result)?;
        }
        Ok(())
    }

    pub fn pre_count(&self)  -> usize { self.pre_hooks.len() }
    pub fn post_count(&self) -> usize { self.post_hooks.len() }
}
