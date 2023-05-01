use std::{fmt::Debug, path::PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::eyre::{Result, WrapErr};
use llm::{
    ElementType, InferenceParameters, InferenceSessionParameters, LoadProgress, Model,
    ModelKVMemoryType, TokenBias,
};
use rand::SeedableRng;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub enum Args {
    /// Use a LLaMA model
    Llama {
        #[command(subcommand)]
        args: BaseArgs,
    },
    /// Use a BLOOM model
    Bloom {
        #[command(subcommand)]
        args: BaseArgs,
    },
    /// Use a GPT-2 model
    Gpt2 {
        #[command(subcommand)]
        args: BaseArgs,
    },
    /// Use a GPT-NeoX model
    #[clap(id = "neox")]
    NeoX {
        #[command(subcommand)]
        args: BaseArgs,
    },
}

#[derive(Subcommand, Debug)]
pub enum BaseArgs {
    #[command()]
    /// Use a model to infer the next tokens in a sequence, and exit.
    Infer(Box<Infer>),

    #[command()]
    /// Get information about a GGML model.
    Info(Box<Info>),

    #[command()]
    /// Dumps the prompt to console and exits, first as a comma-separated list of token IDs
    /// and then as a list of comma-separated string keys and token ID values.
    DumpTokens(Box<DumpTokens>),

    #[command()]
    /// Use a model to interactively prompt it multiple times, while
    /// resetting the context between invocations.
    Repl(Box<Repl>),

    #[command()]
    /// Use a model to interactively generate tokens, and chat with it.
    ///
    /// Note that most, if not all, existing models are not trained for this
    /// and do not support a long enough context window to be able to
    /// have an extended conversation.
    ChatExperimental(Box<Repl>),

    /// Quantize a GGML model to 4-bit.
    Quantize(Box<Quantize>),
}

#[derive(Parser, Debug)]
pub struct Infer {
    #[command(flatten)]
    pub model_load: ModelLoad,

    #[command(flatten)]
    pub prompt_file: PromptFile,

    #[command(flatten)]
    pub generate: Generate,

    /// The prompt to feed the generator.
    ///
    /// If used with `--prompt-file`/`-f`, the prompt from the file will be used
    /// and `{{PROMPT}}` will be replaced with the value of `--prompt`/`-p`.
    #[arg(long, short = 'p', default_value = None)]
    pub prompt: Option<String>,

    /// Saves an inference session at the given path. The same session can then be
    /// loaded from disk using `--load-session`.
    ///
    /// Use this with `-n 0` to save just the prompt
    #[arg(long, default_value = None)]
    pub save_session: Option<PathBuf>,

    /// Loads an inference session from the given path if present, and then saves
    /// the result to the same path after inference is completed.
    ///
    /// Equivalent to `--load-session` and `--save-session` with the same path,
    /// but will not error if the path does not exist
    #[arg(long, default_value = None)]
    pub persist_session: Option<PathBuf>,
}

#[derive(Parser, Debug)]
pub struct Info {
    /// The model to inspect
    #[arg(long, short = 'm')]
    pub model_path: PathBuf,

    /// Whether or not to dump the entire vocabulary
    #[arg(long, short = 'v')]
    pub dump_vocabulary: bool,
}

#[derive(Parser, Debug)]
pub struct DumpTokens {
    #[command(flatten)]
    pub model_load: ModelLoad,

    #[command(flatten)]
    pub prompt_file: PromptFile,

    /// The prompt to feed the generator.
    ///
    /// If used with `--prompt-file`/`-f`, the prompt from the file will be used
    /// and `{{PROMPT}}` will be replaced with the value of `--prompt`/`-p`.
    #[arg(long, short = 'p', default_value = None)]
    pub prompt: Option<String>,
}

#[derive(Parser, Debug)]
pub struct Repl {
    #[command(flatten)]
    pub model_load: ModelLoad,

    #[command(flatten)]
    pub prompt_file: PromptFile,

    #[command(flatten)]
    pub generate: Generate,
}

#[derive(Parser, Debug)]
pub struct Generate {
    /// Sets the number of threads to use
    #[arg(long, short = 't')]
    pub num_threads: Option<usize>,

    /// Sets how many tokens to predict
    #[arg(long, short = 'n')]
    pub num_predict: Option<usize>,

    /// How many tokens from the prompt at a time to feed the network. Does not
    /// affect generation.
    #[arg(long, default_value_t = 8)]
    pub batch_size: usize,

    /// Size of the 'last N' buffer that is used for the `repeat_penalty`
    /// option. In tokens.
    #[arg(long, default_value_t = 64)]
    pub repeat_last_n: usize,

    /// The penalty for repeating tokens. Higher values make the generation less
    /// likely to get into a loop, but may harm results when repetitive outputs
    /// are desired.
    #[arg(long, default_value_t = 1.30)]
    pub repeat_penalty: f32,

    /// Temperature
    #[arg(long, default_value_t = 0.80)]
    pub temperature: f32,

    /// Top-K: The top K words by score are kept during sampling.
    #[arg(long, default_value_t = 40)]
    pub top_k: usize,

    /// Top-p: The cumulative probability after which no more words are kept
    /// for sampling.
    #[arg(long, default_value_t = 0.95)]
    pub top_p: f32,

    /// Loads a saved inference session from the given path, previously saved using
    /// `--save-session`
    #[arg(long, default_value = None)]
    pub load_session: Option<PathBuf>,

    /// Specifies the seed to use during sampling. Note that, depending on
    /// hardware, the same seed may lead to different results on two separate
    /// machines.
    #[arg(long, default_value = None)]
    pub seed: Option<u64>,

    /// Use 16-bit floats for model memory key and value. Ignored when restoring
    /// from the cache.
    #[arg(long, default_value_t = false)]
    pub float16: bool,

    /// A comma separated list of token biases. The list should be in the format
    /// "TID=BIAS,TID=BIAS" where TID is an integer token ID and BIAS is a
    /// floating point number.
    /// For example, "1=-1.0,2=-1.0" sets the bias for token IDs 1
    /// (start of document) and 2 (end of document) to -1.0 which effectively
    /// disables the model from generating responses containing those token IDs.
    #[arg(long, default_value = None, value_parser = parse_bias)]
    pub token_bias: Option<TokenBias>,

    /// Prevent the end of stream (EOS/EOD) token from being generated. This will allow the
    /// model to generate text until it runs out of context space. Note: The --token-bias
    /// option will override this if specified.
    #[arg(long, default_value_t = false)]
    pub ignore_eos: bool,
}
impl Generate {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    pub fn autodetect_num_threads(&self) -> usize {
        std::process::Command::new("sysctl")
            .arg("-n")
            .arg("hw.perflevel0.physicalcpu")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok()?.trim().parse().ok())
            .unwrap_or(num_cpus::get_physical())
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    pub fn autodetect_num_threads(&self) -> usize {
        num_cpus::get_physical()
    }

    pub fn num_threads(&self) -> usize {
        self.num_threads
            .unwrap_or_else(|| self.autodetect_num_threads())
    }

    pub fn inference_session_parameters(&self) -> InferenceSessionParameters {
        let mem_typ = if self.float16 {
            ModelKVMemoryType::Float16
        } else {
            ModelKVMemoryType::Float32
        };
        InferenceSessionParameters {
            memory_k_type: mem_typ,
            memory_v_type: mem_typ,
            repetition_penalty_last_n: self.repeat_last_n,
        }
    }

    pub fn rng(&self) -> rand::rngs::StdRng {
        if let Some(seed) = self.seed {
            rand::rngs::StdRng::seed_from_u64(seed)
        } else {
            rand::rngs::StdRng::from_entropy()
        }
    }

    pub fn inference_parameters(&self, eot: llm::TokenId) -> InferenceParameters {
        InferenceParameters {
            n_threads: self.num_threads(),
            n_batch: self.batch_size,
            top_k: self.top_k,
            top_p: self.top_p,
            repeat_penalty: self.repeat_penalty,
            temperature: self.temperature,
            bias_tokens: self.token_bias.clone().unwrap_or_else(|| {
                if self.ignore_eos {
                    TokenBias::new(vec![(eot, -1.0)])
                } else {
                    TokenBias::default()
                }
            }),
        }
    }
}
fn parse_bias(s: &str) -> Result<TokenBias, String> {
    s.parse()
}

#[derive(Parser, Debug)]
pub struct ModelLoad {
    /// Where to load the model from
    #[arg(long, short = 'm')]
    pub model_path: PathBuf,

    /// Sets the size of the context (in tokens). Allows feeding longer prompts.
    /// Note that this affects memory.
    ///
    /// LLaMA models are trained with a context size of 2048 tokens. If you
    /// want to use a larger context size, you will need to retrain the model,
    /// or use a model that was trained with a larger context size.
    ///
    /// Alternate methods to extend the context, including
    /// [context clearing](https://github.com/rustformers/llm/issues/77) are
    /// being investigated, but are not yet implemented. Additionally, these
    /// will likely not perform as well as a model with a larger context size.
    #[arg(long, default_value_t = 2048)]
    pub num_ctx_tokens: usize,

    /// Don't use mmap to load the model.
    #[arg(long)]
    pub no_mmap: bool,
}
impl ModelLoad {
    pub fn load<M: llm::KnownModel + 'static>(&self) -> Result<Box<dyn Model>> {
        let now = std::time::Instant::now();

        let model = llm::load::<M>(
            &self.model_path,
            !self.no_mmap,
            self.num_ctx_tokens,
            load_progress_handler_log,
        )
        .wrap_err("Could not load model")?;

        log::info!(
            "Model fully loaded! Elapsed: {}ms",
            now.elapsed().as_millis()
        );

        Ok(Box::new(model))
    }
}

pub(crate) fn load_progress_handler_log(progress: LoadProgress) {
    match progress {
        LoadProgress::HyperparametersLoaded => {
            log::debug!("Loaded hyperparameters")
        }
        LoadProgress::ContextSize { bytes } => log::info!(
            "ggml ctx size = {:.2} MB\n",
            bytes as f64 / (1024.0 * 1024.0)
        ),
        LoadProgress::TensorLoaded {
            current_tensor,
            tensor_count,
            ..
        } => {
            let current_tensor = current_tensor + 1;
            if current_tensor % 8 == 0 {
                log::info!("Loaded tensor {current_tensor}/{tensor_count}");
            }
        }
        LoadProgress::Loaded {
            byte_size,
            tensor_count,
        } => {
            log::info!("Loading of model complete");
            log::info!(
                "Model size = {:.2} MB / num tensors = {}",
                byte_size as f64 / 1024.0 / 1024.0,
                tensor_count
            );
        }
    }
}

#[derive(Parser, Debug)]
pub struct PromptFile {
    /// A file to read the prompt from.
    #[arg(long, short = 'f', default_value = None)]
    pub prompt_file: Option<String>,
}
impl PromptFile {
    pub fn contents(&self) -> Option<String> {
        match &self.prompt_file {
            Some(path) => {
                match std::fs::read_to_string(path) {
                    Ok(mut prompt) => {
                        // Strip off the last character if it's exactly newline. Also strip off a single
                        // carriage return if it's there. Since String must be valid UTF-8 it should be
                        // guaranteed that looking at the string as bytes here is safe: UTF-8 non-ASCII
                        // bytes will always the high bit set.
                        if matches!(prompt.as_bytes().last(), Some(b'\n')) {
                            prompt.pop();
                        }
                        if matches!(prompt.as_bytes().last(), Some(b'\r')) {
                            prompt.pop();
                        }
                        Some(prompt)
                    }
                    Err(err) => {
                        log::error!("Could not read prompt file at {path}. Error {err}");
                        std::process::exit(1);
                    }
                }
            }
            _ => None,
        }
    }
}

#[derive(Parser, Debug)]
pub struct Convert {
    /// Path to model directory
    #[arg(long, short = 'd')]
    pub directory: PathBuf,

    /// File type to convert to
    #[arg(long, short = 't', value_enum, default_value_t = FileType::Q4_0)]
    pub file_type: FileType,
}
#[derive(Parser, Debug, ValueEnum, Clone, Copy)]
pub enum FileType {
    /// Quantized 4-bit (type 0).
    Q4_0,
    /// Quantized 4-bit (type 1); used by GPTQ.
    Q4_1,
    /// Float 16-bit.
    F16,
    /// Float 32-bit.
    F32,
}
impl From<FileType> for llm::FileType {
    fn from(t: FileType) -> Self {
        match t {
            FileType::Q4_0 => llm::FileType::MostlyQ4_0,
            FileType::Q4_1 => llm::FileType::MostlyQ4_1,
            FileType::F16 => llm::FileType::MostlyF16,
            FileType::F32 => llm::FileType::F32,
        }
    }
}

#[derive(Parser, Debug)]
pub struct Quantize {
    /// The path to the model to quantize
    #[arg()]
    pub source: PathBuf,

    /// The path to save the quantized model to
    #[arg()]
    pub destination: PathBuf,

    /// The format to convert to
    pub target: QuantizationTarget,
}

#[derive(Parser, Debug, ValueEnum, Clone, Copy)]
#[clap(rename_all = "snake_case")]
pub enum QuantizationTarget {
    /// Quantized 4-bit (type 0).
    Q4_0,
    /// Quantized 4-bit (type 1).
    Q4_1,
}
impl From<QuantizationTarget> for ElementType {
    fn from(t: QuantizationTarget) -> Self {
        match t {
            QuantizationTarget::Q4_0 => ElementType::Q4_0,
            QuantizationTarget::Q4_1 => ElementType::Q4_1,
        }
    }
}