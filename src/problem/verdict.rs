use multiprocessing::Object;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;

// pub enum SubmissionVerdict {
//     InQueue,
//     Compiling,
//     CompilationError(String),
//     Running(u64),

//     Accepted,
//     PartialSolution(u64), // in 10000 increments

//     Ignored,
//     Rejected,
//     Disqualified,

//     Bug(String),

//     WrongAnswer(u64),
//     RuntimeError(u64),
//     TimeLimitExceeded(u64),
//     MemoryLimitExceeded(u64),
//     PresentationError(u64),
//     IdlenessLimitExceeded(u64),
//     // This one indicates the checker returning FL, not the checker crashing or something -- that
//     // would be Bug.
//     CheckerFailed(u64),
// }

#[derive(Object, Debug, Serialize, Clone)]
pub enum TestVerdict {
    InQueue,
    Running,
    Ignored,

    Accepted,
    PartialSolution(u64), // in 10000 increments

    Bug(String),

    WrongAnswer,
    RuntimeError(ExitStatus),
    TimeLimitExceeded,
    MemoryLimitExceeded,
    PresentationError,
    IdlenessLimitExceeded,
    CheckerFailed,
}

#[derive(Object, Debug, Serialize)]
pub struct TestJudgementResult {
    pub verdict: TestVerdict,
    pub logs: HashMap<String, Vec<u8>>,
    pub invocation_stats: HashMap<String, InvocationStat>,
}

#[derive(Object, Debug, Serialize, Clone)]
pub struct InvocationStat {
    pub real_time: std::time::Duration,
    pub cpu_time: std::time::Duration,
    pub user_time: std::time::Duration,
    pub sys_time: std::time::Duration,
    pub memory: usize,
}

#[derive(Object, Debug, Deserialize, Clone)]
pub struct InvocationLimit {
    pub real_time: std::time::Duration,
    pub cpu_time: std::time::Duration,
    pub memory: usize,
}

#[derive(Object, PartialEq, Eq, Debug, Clone, Copy, Serialize)]
pub enum ExitStatus {
    ExitCode(u8),
    Signal(u8),
}

impl From<std::process::ExitStatus> for ExitStatus {
    fn from(status: std::process::ExitStatus) -> Self {
        if let Some(signal) = status.signal() {
            Self::Signal(signal as u8)
        } else {
            Self::ExitCode(status.code().unwrap() as u8) // presumably never fails
        }
    }
}

// Extracted from testlib source code. The first enum is the most complete one and the one testlib
// uses by default, so that's what we use. The other two variaants may be supported later for
// compatibility.

// enum CodeforcesExitCode {
//     Accepted = 0,
//     WrongAnswer = 1,
//     PresentationError = 2,
//     CheckerFailed = 3,
//     Dirt = 4,  // something like 'expected EOF, found data'
//     PartialSolution = 7,
//     UnexpectedEOF = 8,
// }

// enum EjudgeExitCode {
//     Accepted = 0,
//     WrongAnswer = 5,
//     PresentationError = 4,
//     CheckerFailed = 6,
//     Dirt = 6,
// }

// enum ContesterExitCode {
//     Accepted = 0xAC,
//     WrongAnswer = 0xAB,
//     PresentationError = 0xAA,
//     CheckerFailed = 0xA3,
// }

// Short verdict codes taken from https://ejudge.ru/wiki/index.php/Вердикты_тестирования, except AC
// which replaced ejudge's OK verdict because of its popularity. ejudge's understanding of AC as
// "Accepted for testing" is not supported.

// The codes for Bug and CheckerFailed are difficult to get right.
// - In testlib, CheckerFailed is FL and Bug is nonexistent, because that's an unexpected condition.
// - In ejudge, there's "Check Failed" for CF, which is both Bug and CheckerFailed.
// The good thing about standards is there are so many to choose from, so we use FL for
// CheckerFailed and CF for Bug because that's probably what most users would expect.

// impl SubmissionVerdict {
//     fn to_short_string(&self) -> String {
//         match self {
//             Self::InQueue => "PD".to_string(),
//             Self::Compiling => "CG".to_string(),
//             Self::CompilationError(_) => "CE".to_string(),
//             Self::Running(test) => format!("RU {test}"),

//             Self::Accepted => "AC".to_string(),
//             Self::PartialSolution(points) => format!("PT {}", (*points as f64) / 10000.0),

//             Self::Ignored => "IG".to_string(),
//             Self::Rejected => "RJ".to_string(),
//             Self::Disqualified => "DQ".to_string(),

//             Self::Bug(_) => "CF".to_string(),

//             Self::WrongAnswer(test) => format!("WA {test}"),
//             Self::RuntimeError(test) => format!("RE {test}"),
//             Self::TimeLimitExceeded(test) => format!("TL {test}"),
//             Self::MemoryLimitExceeded(test) => format!("ML {test}"),
//             Self::PresentationError(test) => format!("PE {test}"),
//             Self::IdlenessLimitExceeded(test) => format!("IL {test}"),
//             Self::CheckerFailed(test) => format!("FL {test}"),
//         }
//     }
// }

impl TestVerdict {
    pub fn to_short_string(&self) -> String {
        match self {
            Self::InQueue => "PD".to_string(),
            Self::Running => "RU".to_string(),
            Self::Ignored => "IG".to_string(),

            Self::Accepted => "AC".to_string(),
            Self::PartialSolution(points) => format!("PT {}", (*points as f64) / 10000.0),

            Self::Bug(_) => "CF".to_string(),

            Self::WrongAnswer => "WA".to_string(),
            Self::RuntimeError(_) => "RE".to_string(),
            Self::TimeLimitExceeded => "TL".to_string(),
            Self::MemoryLimitExceeded => "ML".to_string(),
            Self::PresentationError => "PE".to_string(),
            Self::IdlenessLimitExceeded => "IL".to_string(),
            Self::CheckerFailed => "FL".to_string(),
        }
    }

    pub fn is_successful(&self) -> bool {
        match self {
            Self::InQueue => panic!("Unexpected verdict"),
            Self::Running => panic!("Unexpected verdict"),
            Self::Ignored => false,

            Self::Accepted => true,
            Self::PartialSolution(_) => true,

            Self::Bug(_) => false,

            Self::WrongAnswer => false,
            Self::RuntimeError(_) => false,
            Self::TimeLimitExceeded => false,
            Self::MemoryLimitExceeded => false,
            Self::PresentationError => false,
            Self::IdlenessLimitExceeded => false,
            Self::CheckerFailed => false,
        }
    }

    pub fn from_testlib(status: ExitStatus, stderr: &[u8]) -> Self {
        match status {
            ExitStatus::ExitCode(code) => match code {
                0 => Self::Accepted,
                1 => Self::WrongAnswer,
                2 => Self::PresentationError,
                3 => Self::CheckerFailed,
                4 => Self::PresentationError,
                7 => {
                    if !stderr.starts_with(b"points ") {
                        return Self::Bug(
                            "Testlib exit code is 7 (PT), but stderr does not start with 'points '"
                                .to_string(),
                        );
                    }

                    let mut points = &stderr[7..];

                    if let Some(idx) = stderr[7..]
                        .iter()
                        .position(|c| *c == b' ' || *c == b'\r' || *c == b'\n' || *c == b'\t')
                    {
                        points = &points[..idx];
                    }

                    let points = match std::str::from_utf8(points) {
                        Ok(points) => points,
                        Err(e) => {
                            return Self::Bug(format!("{points:?} is not a UTF-8 string: {e:?}"))
                        }
                    };

                    let points: f64 = match points.parse() {
                        Ok(points) => points,
                        Err(_) => {
                            return Self::Bug(format!(
                                "Testlib exit code is 7 (PT) and stderr starts with 'points \
                                 {points}', but '{points}' is not a number convertible to f64"
                            ))
                        }
                    };

                    if !points.is_finite() {
                        return Self::Bug(format!(
                            "Partial result must be a finite number, not {points}"
                        ));
                    }

                    Self::PartialSolution((points * 10000.0).round() as u64)
                }
                8 => Self::PresentationError,
                _ => Self::Bug(format!("Unknown testlib exit code: {code}")),
            },
            ExitStatus::Signal(signal) => {
                Self::Bug(format!("Testlib terminated by signal {signal}"))
            }
        }
    }
}
