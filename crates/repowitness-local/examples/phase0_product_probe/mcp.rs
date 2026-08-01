use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::ProbeResult;

const IO_DEADLINE: Duration = Duration::from_secs(10);
// Tool schemas are static protocol metadata rather than material evidence. The
// default surface is still bounded below, while a single tools/list response
// needs room for every supported read-only schema.
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_STDERR_BYTES: u64 = 16 * 1024;

pub struct McpMetrics {
    pub tool_count: usize,
    pub material_result_bytes: usize,
}

pub fn probe_default_surface(
    cli: &Path,
    repository: &Path,
    database: &Path,
    repository_identity: &str,
    max_material_result_bytes: usize,
) -> ProbeResult<McpMetrics> {
    let mut process = McpProcess::start(cli, repository, database, repository_identity)?;
    let result = probe(&mut process, max_material_result_bytes);
    let stopped = process.stop();
    match (result, stopped) {
        (Ok(metrics), Ok(())) => Ok(metrics),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn probe(process: &mut McpProcess, max_material_result_bytes: usize) -> ProbeResult<McpMetrics> {
    let initialized = process.request(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "repowitness-phase0-probe", "version": "1"}
        }
    }))?;
    if initialized["result"]["protocolVersion"] != json!("2025-11-25") {
        return Err("MCP initialization did not negotiate the expected protocol".into());
    }
    process.notification(json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }))?;

    let listed = process.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    }))?;
    let tools = listed["result"]["tools"]
        .as_array()
        .ok_or("MCP tools/list did not return an array")?;
    let names = tools
        .iter()
        .map(|tool| {
            tool["name"]
                .as_str()
                .ok_or("MCP tool name was not a string")
        })
        .collect::<Result<Vec<_>, _>>()?;
    const REQUIRED_BASELINE_TOOLS: [&str; 5] = [
        "code_search",
        "context_build",
        "diagnostics",
        "memory_recall",
        "symbol_get",
    ];
    if names.len() > 32
        || names.windows(2).any(|pair| pair[0] >= pair[1])
        || REQUIRED_BASELINE_TOOLS
            .iter()
            .any(|required| !names.contains(required))
        || names
            .iter()
            .any(|name| matches!(*name, "memory_manage" | "personal_memory"))
    {
        return Err("default MCP capabilities were not bounded and read-only".into());
    }

    let searched = process.request(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "code_search",
            "arguments": {"query": "into_frame", "max_results": 20}
        }
    }))?;
    if searched["result"]["isError"] == json!(true) {
        return Err("MCP code_search returned a tool error".into());
    }
    let material = &searched["result"]["structuredContent"];
    let material_bytes = serde_json::to_vec(material)?.len();
    if material_bytes > max_material_result_bytes {
        return Err("MCP material result exceeded the proposed output budget".into());
    }
    let matches = material["matches"]
        .as_array()
        .ok_or("MCP code_search did not return matches")?;
    let found = matches.iter().any(|candidate| {
        candidate["name"] == json!("into_frame")
            && candidate["qualified_name"] == json!("Set::into_frame")
    });
    if !found {
        return Err("MCP code_search omitted the pinned SET evidence".into());
    }
    Ok(McpMetrics {
        tool_count: names.len(),
        material_result_bytes: material_bytes,
    })
}

struct McpProcess {
    child: Option<Child>,
    input: Option<ChildStdin>,
    responses: Receiver<Result<Vec<u8>, &'static str>>,
    reader: Option<JoinHandle<()>>,
    stderr: Receiver<Vec<u8>>,
    stderr_reader: Option<JoinHandle<()>>,
}

impl McpProcess {
    fn start(
        cli: &Path,
        repository: &Path,
        database: &Path,
        repository_identity: &str,
    ) -> ProbeResult<Self> {
        let mut child = Command::new(cli)
            .args([
                "mcp-serve",
                "--repository-id",
                repository_identity,
                "--database",
            ])
            .arg(database)
            .arg("--root")
            .arg(repository)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take().ok_or("MCP stdin was unavailable")?;
        let stdout = child.stdout.take().ok_or("MCP stdout was unavailable")?;
        let stderr = child.stderr.take().ok_or("MCP stderr was unavailable")?;

        let (response_sender, responses) = mpsc::sync_channel(4);
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                let read = reader
                    .by_ref()
                    .take(u64::try_from(MAX_MESSAGE_BYTES + 1).expect("fixed bound"))
                    .read_until(b'\n', &mut line);
                match read {
                    Ok(0) => break,
                    Ok(_) if line.len() <= MAX_MESSAGE_BYTES && line.last() == Some(&b'\n') => {
                        if response_sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {
                        let _ = response_sender.send(Err("MCP response exceeded its line bound"));
                        break;
                    }
                    Err(_) => {
                        let _ = response_sender.send(Err("MCP response could not be read"));
                        break;
                    }
                }
            }
        });

        let (stderr_sender, stderr_receiver) = mpsc::sync_channel(1);
        let stderr_reader = thread::spawn(move || {
            let mut bounded = stderr.take(MAX_STDERR_BYTES + 1);
            let mut bytes = Vec::new();
            let _ = bounded.read_to_end(&mut bytes);
            let _ = stderr_sender.send(bytes);
        });
        Ok(Self {
            child: Some(child),
            input: Some(input),
            responses,
            reader: Some(reader),
            stderr: stderr_receiver,
            stderr_reader: Some(stderr_reader),
        })
    }

    fn request(&mut self, value: Value) -> ProbeResult<Value> {
        self.write_message(value)?;
        let line = self
            .responses
            .recv_timeout(IO_DEADLINE)
            .map_err(|_| "MCP response deadline elapsed")??;
        Ok(serde_json::from_slice(&line)?)
    }

    fn notification(&mut self, value: Value) -> ProbeResult<()> {
        self.write_message(value)
    }

    fn write_message(&mut self, value: Value) -> ProbeResult<()> {
        let mut encoded = serde_json::to_vec(&value)?;
        if encoded.len() >= MAX_MESSAGE_BYTES {
            return Err("MCP request exceeded its line bound".into());
        }
        encoded.push(b'\n');
        let input = self.input.as_mut().ok_or("MCP stdin was closed")?;
        input.write_all(&encoded)?;
        input.flush()?;
        Ok(())
    }

    fn stop(mut self) -> ProbeResult<()> {
        self.input.take();
        let deadline = Instant::now()
            .checked_add(IO_DEADLINE)
            .ok_or("MCP shutdown deadline is not representable")?;
        let mut child = self.child.take().ok_or("MCP child was unavailable")?;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                let _ = child.wait();
                return Err("MCP shutdown deadline elapsed".into());
            }
            thread::sleep(Duration::from_millis(10));
        };
        if let Some(reader) = self.reader.take() {
            reader.join().map_err(|_| "MCP stdout reader panicked")?;
        }
        let stderr = self
            .stderr
            .recv_timeout(IO_DEADLINE)
            .map_err(|_| "MCP stderr reader deadline elapsed")?;
        if let Some(reader) = self.stderr_reader.take() {
            reader.join().map_err(|_| "MCP stderr reader panicked")?;
        }
        if !status.success() || !stderr.is_empty() {
            return Err("MCP server did not stop cleanly".into());
        }
        Ok(())
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        self.input.take();
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
