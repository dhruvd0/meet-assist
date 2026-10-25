use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum TesterSpeaker {
    Me,
    Them,
}

impl TesterSpeaker {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Me => "me",
            Self::Them => "them",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct DetectionScores {
    pub loop_score: f64,
    pub stuck: f64,
    pub info: f64,
    pub idea: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TesterTraceKind {
    Detecting,
    Detection {
        outcome: String,
        kind: Option<String>,
        scores: DetectionScores,
    },
    DetectorFallback {
        reason: String,
    },
    DetectorBypassed {
        reason: String,
    },
    Judging,
    Judged {
        outcome: String,
        reason: String,
        remaining_ms: i64,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct TesterTrace {
    pub meeting_id: i64,
    pub session_id: i64,
    pub ts: i64,
    pub kind: TesterTraceKind,
}
