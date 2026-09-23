"""
VeilSim Julia Worker — maps each Veil category to a real numeric computation.

Each exported function returns:
  Dict("success"    => Bool,
       "energy_j"   => Float64,
       "risk_score" => Float64,   # 0.0–1.0
       "duration_s" => Float64,
       "metrics"    => Dict{String,Any})

Called from Python via PyJulia / subprocess JSON bridge.
"""

using LinearAlgebra
using Statistics
using Random
using FFTW      # stdlib via Pkg; fall back to pure-Julia DFT if unavailable
using JSON3

# ── Utility ───────────────────────────────────────────────────────────────────

function clamp01(x::Float64)::Float64
    clamp(x, 0.0, 1.0)
end

# ── 1. control_systems — PID drone stabilisation ─────────────────────────────
"""
Simulate a discrete PID loop stabilising a 1-DOF angle θ.
Wind shear is modelled as coloured noise added to the plant.
Returns energy (integral of |u|²), risk (max deviation / π), duration.
"""
function pid_controller(;
    Kp::Float64 = 1.8,
    Ki::Float64 = 0.05,
    Kd::Float64 = 0.4,
    dt::Float64 = 0.01,
    T::Float64  = 5.0,
    seed::Int   = 42,
)::Dict{String,Any}
    rng   = MersenneTwister(seed)
    steps = round(Int, T / dt)
    θ     = 0.3          # initial deviation (rad)
    dθ    = 0.0
    integral = 0.0
    prev_err = θ

    energy  = 0.0
    max_dev = abs(θ)
    rms_acc = 0.0

    for _ in 1:steps
        err       = -θ                          # setpoint = 0
        integral += err * dt
        deriv     = (err - prev_err) / dt
        u         = Kp*err + Ki*integral + Kd*deriv
        wind      = 0.05 * randn(rng)           # wind shear
        dθ        = dθ + (u + wind) * dt
        θ         = θ + dθ * dt
        prev_err  = err

        energy  += u^2 * dt
        rms_acc += θ^2
        max_dev  = max(max_dev, abs(θ))
    end

    rms = sqrt(rms_acc / steps)
    success = rms < 0.035  # threshold: 2° ≈ 0.035 rad

    Dict(
        "success"   => success,
        "energy_j"  => energy,
        "risk_score" => clamp01(max_dev / π),
        "duration_s" => T,
        "metrics"   => Dict{String,Any}(
            "rms_rad"   => rms,
            "max_dev_rad" => max_dev,
            "Kp" => Kp, "Ki" => Ki, "Kd" => Kd,
        ),
    )
end

# ── 2. machine_learning — gradient descent (training loss curve) ──────────────
"""
Simulate SGD on a quadratic loss L(w) = ||w - w*||² + λ||w||².
Returns energy proxy (sum of gradient norms), risk (final loss), duration.
"""
function gradient_descent(;
    dim::Int    = 32,
    lr::Float64 = 0.01,
    epochs::Int = 200,
    seed::Int   = 0,
)::Dict{String,Any}
    rng  = MersenneTwister(seed)
    w    = randn(rng, dim)
    wstar = randn(rng, dim)
    λ    = 1e-3

    grad_energy = 0.0
    losses      = Float64[]
    t0 = time()

    for _ in 1:epochs
        g     = 2.0 .* (w .- wstar) .+ 2λ .* w
        g    .+= 0.01 .* randn(rng, dim)   # SGD noise
        w   .-= lr .* g
        loss   = sum((w .- wstar).^2) + λ * sum(w.^2)
        push!(losses, loss)
        grad_energy += norm(g)^2
    end

    final_loss = losses[end]
    converged  = final_loss < 0.5

    Dict(
        "success"    => converged,
        "energy_j"   => grad_energy,
        "risk_score" => clamp01(final_loss / 10.0),
        "duration_s" => time() - t0,
        "metrics"    => Dict{String,Any}(
            "final_loss"   => final_loss,
            "initial_loss" => losses[1],
            "epochs"       => epochs,
            "converged"    => converged,
        ),
    )
end

# ── 3. signal_processing — FFT spectral analysis ─────────────────────────────
"""
Generate a synthetic multi-tone signal + noise, run FFT, recover dominant freqs.
Risk = fraction of noise power vs total.
"""
function fft_analysis(;
    fs::Float64      = 1000.0,
    T::Float64       = 1.0,
    freqs::Vector{Float64} = [50.0, 120.0, 300.0],
    snr_db::Float64  = 20.0,
    seed::Int        = 7,
)::Dict{String,Any}
    rng  = MersenneTwister(seed)
    N    = round(Int, fs * T)
    t    = range(0, T; length=N)
    sig  = sum(sin.(2π .* f .* t) for f in freqs)
    noise_amp = 10^(-snr_db / 20.0)
    noisy = sig .+ noise_amp .* randn(rng, N)

    t0  = time()
    S   = abs.(rfft(noisy)) ./ N
    dur = time() - t0

    # Find peaks
    freqs_axis = rfftfreq(N, fs)
    peak_power = maximum(S)
    noise_power = mean(S)
    snr_actual  = peak_power / max(noise_power, 1e-12)

    Dict(
        "success"    => snr_actual > 5.0,
        "energy_j"   => sum(S.^2),
        "risk_score" => clamp01(1.0 / max(snr_actual, 1.0)),
        "duration_s" => dur,
        "metrics"    => Dict{String,Any}(
            "snr_actual"   => snr_actual,
            "peak_power"   => peak_power,
            "n_samples"    => N,
            "target_freqs" => freqs,
        ),
    )
end

# ── 4. robotics — forward kinematics (6-DOF serial arm) ──────────────────────
"""
Denavit-Hartenberg forward kinematics for a 6-DOF planar arm.
Risk = end-effector distance from target / max_reach.
"""
function forward_kinematics(;
    link_lengths::Vector{Float64} = [0.3, 0.28, 0.22, 0.18, 0.12, 0.08],
    target::Vector{Float64}       = [0.5, 0.3, 0.2],
    seed::Int = 3,
)::Dict{String,Any}
    rng    = MersenneTwister(seed)
    n      = length(link_lengths)
    joints = π .* (rand(rng, n) .- 0.5)  # random joint angles ∈ [-π/2, π/2]

    # Planar DH: accumulate rotation + translation
    pos = zeros(3)
    R   = Matrix{Float64}(I, 3, 3)
    t0  = time()
    for i in 1:n
        θ = joints[i]
        Ri = [cos(θ) -sin(θ) 0;
              sin(θ)  cos(θ) 0;
              0       0      1]
        di = [link_lengths[i]; 0.0; 0.0]
        pos .+= R * di
        R    = R * Ri
    end
    dur = time() - t0

    max_reach = sum(link_lengths)
    dist      = norm(pos .- target)
    success   = dist < 0.05

    Dict(
        "success"    => success,
        "energy_j"   => sum(abs.(joints)) * 10.0,  # proportional to torque
        "risk_score" => clamp01(dist / max_reach),
        "duration_s" => dur,
        "metrics"    => Dict{String,Any}(
            "end_effector" => pos,
            "dist_to_target" => dist,
            "joint_angles"   => joints,
            "dof"            => n,
        ),
    )
end

# ── 5. computer_vision — 2D convolution (edge detection) ─────────────────────
"""
Apply a Sobel edge-detection kernel to a synthetic grayscale image.
Energy = mean gradient magnitude. Risk = clutter ratio.
"""
function image_convolution(;
    H::Int   = 64,
    W::Int   = 64,
    seed::Int = 11,
)::Dict{String,Any}
    rng = MersenneTwister(seed)
    # Synthetic image: rectangles on noise
    img = 0.1 .* rand(rng, H, W)
    img[20:44, 20:44] .+= 0.8
    img[10:20, 50:60] .+= 0.6

    Gx = [-1 0 1; -2 0 2; -1 0 1]
    Gy = [-1 -2 -1; 0 0 0; 1 2 1]

    t0    = time()
    edges = zeros(H-2, W-2)
    @inbounds for i in 2:H-1, j in 2:W-1
        patch = img[i-1:i+1, j-1:j+1]
        gx = sum(Gx .* patch)
        gy = sum(Gy .* patch)
        edges[i-1, j-1] = sqrt(gx^2 + gy^2)
    end
    dur = time() - t0

    mean_grad  = mean(edges)
    edge_ratio = count(e -> e > 0.3, edges) / length(edges)

    Dict(
        "success"    => mean_grad > 0.05,
        "energy_j"   => sum(edges),
        "risk_score" => clamp01(edge_ratio),
        "duration_s" => dur,
        "metrics"    => Dict{String,Any}(
            "mean_gradient" => mean_grad,
            "edge_ratio"    => edge_ratio,
            "image_size"    => [H, W],
        ),
    )
end

# ── 6. iot_networks — gossip protocol simulation ──────────────────────────────
"""
Simulate synchronous gossip on N nodes for R rounds.
A node with a message randomly selects a neighbour to infect.
Energy = total messages sent. Risk = 1 - coverage fraction.
"""
function gossip_protocol(;
    N::Int     = 32,
    rounds::Int = 12,
    fanout::Int = 2,
    seed::Int  = 99,
)::Dict{String,Any}
    rng      = MersenneTwister(seed)
    infected = Set{Int}([1])  # node 1 starts with the message
    msgs_sent = 0
    t0        = time()

    for _ in 1:rounds
        new_inf = Set{Int}()
        for node in infected
            targets = rand(rng, 1:N, fanout)
            for t in targets
                msgs_sent += 1
                push!(new_inf, t)
            end
        end
        union!(infected, new_inf)
        length(infected) == N && break
    end

    coverage = length(infected) / N
    dur      = time() - t0

    Dict(
        "success"    => coverage > 0.9,
        "energy_j"   => Float64(msgs_sent) * 0.001,   # 1mJ per message
        "risk_score" => clamp01(1.0 - coverage),
        "duration_s" => dur,
        "metrics"    => Dict{String,Any}(
            "coverage_frac" => coverage,
            "msgs_sent"     => msgs_sent,
            "nodes"         => N,
            "rounds"        => rounds,
        ),
    )
end

# ── 7. optimization — Rosenbrock gradient descent ────────────────────────────
"""
Minimise the Rosenbrock banana function f(x,y) = (1-x)² + 100(y-x²)².
Uses vanilla gradient descent with adaptive step.
Risk = log10(final_val + 1) / 5.
"""
function rosenbrock_optimize(;
    lr::Float64   = 1e-3,
    iters::Int    = 2000,
    x0::Float64   = -1.2,
    y0::Float64   = 1.0,
    seed::Int     = 0,  # unused, deterministic
)::Dict{String,Any}
    x, y  = x0, y0
    grad_energy = 0.0
    t0    = time()

    for _ in 1:iters
        gx = -2(1 - x) - 400x*(y - x^2)
        gy =  200(y - x^2)
        x -= lr * gx
        y -= lr * gy
        grad_energy += gx^2 + gy^2
    end

    final_val = (1 - x)^2 + 100*(y - x^2)^2
    dur       = time() - t0

    Dict(
        "success"    => final_val < 0.01,
        "energy_j"   => grad_energy * 1e-6,
        "risk_score" => clamp01(log10(final_val + 1) / 5.0),
        "duration_s" => dur,
        "metrics"    => Dict{String,Any}(
            "final_val"   => final_val,
            "final_x"     => x,
            "final_y"     => y,
            "iters"       => iters,
        ),
    )
end

# ── Dispatch table ────────────────────────────────────────────────────────────

const CATEGORY_FN = Dict{String, Function}(
    "control_systems"   => (kwargs...) -> pid_controller(; kwargs...),
    "machine_learning"  => (kwargs...) -> gradient_descent(; kwargs...),
    "signal_processing" => (kwargs...) -> fft_analysis(; kwargs...),
    "robotics"          => (kwargs...) -> forward_kinematics(; kwargs...),
    "computer_vision"   => (kwargs...) -> image_convolution(; kwargs...),
    "iot_networks"      => (kwargs...) -> gossip_protocol(; kwargs...),
    "optimization"      => (kwargs...) -> rosenbrock_optimize(; kwargs...),
)

"""
Main entry point: run a Veil by category name.
Accepts optional keyword args forwarded to the specific worker.
Returns JSON-serialisable Dict.
"""
function run_veil(category::String; kwargs...)::Dict{String,Any}
    fn = get(CATEGORY_FN, category, nothing)
    if fn === nothing
        @warn "Unknown category '$category', falling back to optimization"
        fn = rosenbrock_optimize
    end
    fn(; kwargs...)
end

# ── CLI / stdin bridge ────────────────────────────────────────────────────────
# When invoked as a subprocess, read one JSON line from stdin and write result to stdout.
# Format: {"category": "control_systems", "seed": 42, ...}

if abspath(PROGRAM_FILE) == @__FILE__
    input = JSON3.read(readline(stdin), Dict{String,Any})
    category = get(input, "category", "optimization")
    # Pass remaining keys as kwargs (string keys converted to Symbol)
    kwargs = Dict(Symbol(k) => v for (k, v) in input if k != "category")
    result = run_veil(category; kwargs...)
    println(JSON3.write(result))
end
