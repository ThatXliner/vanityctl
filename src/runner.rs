use std::{
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[String], cwd: Option<&Path>) -> Result<CommandOutput>;
}

#[derive(Default)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[String], cwd: Option<&Path>) -> Result<CommandOutput> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let output = command.output().with_context(|| format!("run {program}"))?;
        let result = CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            code: output.status.code().unwrap_or(-1),
        };
        if !output.status.success() {
            bail!(
                "{} {} failed ({}): {}",
                program,
                args.join(" "),
                result.code,
                result.stderr.trim()
            );
        }
        Ok(result)
    }
}

#[derive(Default)]
pub struct RecordingRunner {
    pub calls: Mutex<Vec<(String, Vec<String>)>>,
    pub response: Mutex<Option<CommandOutput>>,
}

impl CommandRunner for RecordingRunner {
    fn run(&self, program: &str, args: &[String], _cwd: Option<&Path>) -> Result<CommandOutput> {
        self.calls
            .lock()
            .unwrap()
            .push((program.into(), args.to_vec()));
        Ok(self
            .response
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
                code: 0,
            }))
    }
}

pub type SharedRunner = Arc<dyn CommandRunner>;
