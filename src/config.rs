use clap::ValueEnum;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BackendProfile {
    Chutes,
    Deepseek,
    OpenaiGeneric,
}

impl BackendProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            BackendProfile::Chutes => "chutes",
            BackendProfile::Deepseek => "deepseek",
            BackendProfile::OpenaiGeneric => "openai_generic",
        }
    }

    pub fn supports_top_k(self) -> bool {
        matches!(self, BackendProfile::Chutes)
    }

    pub fn supports_reasoning(self) -> bool {
        matches!(self, BackendProfile::Chutes | BackendProfile::Deepseek)
    }

    pub fn max_tool_name_len(self) -> Option<usize> {
        match self {
            BackendProfile::Deepseek => Some(64),
            _ => None,
        }
    }
}

impl FromStr for BackendProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chutes" => Ok(Self::Chutes),
            "deepseek" | "deep-seek" => Ok(Self::Deepseek),
            "openai_generic" | "openai-generic" | "openai" | "generic" => Ok(Self::OpenaiGeneric),
            other => Err(format!(
                "unsupported backend profile '{other}', expected 'chutes', 'deepseek' or 'openai_generic'"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum UpstreamApi {
    Responses,
    ChatCompletions,
}

impl UpstreamApi {
    pub fn as_str(self) -> &'static str {
        match self {
            UpstreamApi::Responses => "responses",
            UpstreamApi::ChatCompletions => "chat_completions",
        }
    }
}

impl FromStr for UpstreamApi {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "responses" | "response" => Ok(Self::Responses),
            "chat_completions" | "chat-completions" | "chat" => Ok(Self::ChatCompletions),
            other => Err(format!(
                "unsupported upstream api '{other}', expected 'responses' or 'chat_completions'"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CompatMode {
    Strict,
    Compat,
}

impl CompatMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CompatMode::Strict => "strict",
            CompatMode::Compat => "compat",
        }
    }

    pub fn is_strict(self) -> bool {
        matches!(self, CompatMode::Strict)
    }
}

impl FromStr for CompatMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "strict" => Ok(Self::Strict),
            "compat" | "compatible" => Ok(Self::Compat),
            other => Err(format!(
                "unsupported compat mode '{other}', expected 'strict' or 'compat'"
            )),
        }
    }
}
