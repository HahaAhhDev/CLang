use crate::error::ClangError;
use std::fs;
use std::os::unix::fs::PermissionsExt;

const XOR_KEY: u8 = 0x5A;

pub fn b64_encode(data: &[u8]) -> String {
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHA[((n >> 18) & 63) as usize] as char);
        out.push(ALPHA[((n >> 12) & 63) as usize] as char);
        if i + 1 < data.len() {
            out.push(ALPHA[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < data.len() {
            out.push(ALPHA[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

pub fn b64_decode(text: &str) -> Result<Vec<u8>, ClangError> {
    let mut vals: Vec<u8> = Vec::new();
    for c in text.chars() {
        if c.is_whitespace() {
            continue;
        }
        let v = match c {
            'A'..='Z' => c as u8 - b'A',
            'a'..='z' => c as u8 - b'a' + 26,
            '0'..='9' => c as u8 - b'0' + 52,
            '+' => 62,
            '/' => 63,
            '=' => 64,
            _ => {
                return Err(ClangError::new(
                    1,
                    "compiled file is damaged".to_string(),
                    "recompile the source file".to_string(),
                ))
            }
        };
        vals.push(v);
    }
    if vals.len() % 4 != 0 {
        return Err(ClangError::new(
            1,
            "compiled file is damaged".to_string(),
            "recompile the source file".to_string(),
        ));
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < vals.len() {
        let a = vals[i] as u32;
        let b = vals[i + 1] as u32;
        let c = vals[i + 2] as u32;
        let d = vals[i + 3] as u32;
        if a == 64 || b == 64 {
            return Err(ClangError::new(
                1,
                "compiled file is damaged".to_string(),
                "recompile the source file".to_string(),
            ));
        }
        let n = (a << 18) | (b << 12) | ((c & 63) << 6) | (d & 63);
        out.push(((n >> 16) & 255) as u8);
        if c != 64 {
            out.push(((n >> 8) & 255) as u8);
        }
        if d != 64 {
            out.push((n & 255) as u8);
        }
        i += 4;
    }
    Ok(out)
}

fn xor_data(data: &[u8]) -> Vec<u8> {
    data.iter().map(|b| b ^ XOR_KEY).collect()
}

pub fn obfuscate(src: &str) -> String {
    let xored = xor_data(src.as_bytes());
    b64_encode(&xored)
}

pub fn deobfuscate(payload: &str) -> Result<String, ClangError> {
    let raw = b64_decode(payload)?;
    let plain = xor_data(&raw);
    String::from_utf8(plain).map_err(|_| {
        ClangError::new(
            1,
            "compiled file is damaged".to_string(),
            "recompile the source file".to_string(),
        )
    })
}

pub fn compile_source(src_path: &str, out_path: &str) -> Result<(), ClangError> {
    let src = fs::read_to_string(src_path).map_err(|e| {
        ClangError::new(
            1,
            format!("cannot read '{}': {}", src_path, e),
            "check the file path".to_string(),
        )
    })?;
    let payload = obfuscate(&src);
    let body = format!("CLNGB1\n{}", payload);
    fs::write(out_path, &body).map_err(|e| {
        ClangError::new(
            1,
            format!("cannot write '{}': {}", out_path, e),
            "check the output folder".to_string(),
        )
    })?;
    let exe_path = out_path.trim_end_matches(".clngb");
    let runner = format!(
        "#!/bin/sh\n# generated launcher, payload lives in {}\nSELF=$(readlink -f \"$0\" 2>/dev/null || realpath \"$0\" 2>/dev/null || echo \"$0\")\nDIR=$(dirname \"$SELF\")\nBASE=$(basename \"$SELF\")\nif [ -f \"$DIR/$BASE.clngb\" ]; then PAYLOAD=\"$DIR/$BASE.clngb\"; elif [ -f \"$BASE.clngb\" ]; then PAYLOAD=\"$BASE.clngb\"; else echo \"missing payload\" >&2; exit 1; fi\nclng \"$PAYLOAD\"\n",
        out_path
    );
    let launch_name = if exe_path == out_path {
        format!("{}.run", out_path)
    } else {
        exe_path.to_string()
    };
    fs::write(&launch_name, runner).map_err(|e| {
        ClangError::new(
            1,
            format!("cannot write launcher '{}': {}", launch_name, e),
            "check the output folder".to_string(),
        )
    })?;
    let mut perms = fs::metadata(&launch_name)
        .map_err(|e| ClangError::new(1, e.to_string(), "check the file".to_string()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&launch_name, perms).map_err(|e| {
        ClangError::new(1, e.to_string(), "chmod failed".to_string())
    })?;
    println!("wrote {} and launcher {}", out_path, launch_name);
    println!("run with: chmod +x {} && ./{}", launch_name, launch_name);
    Ok(())
}

pub fn load_maybe_compiled(path: &str) -> Result<String, ClangError> {
    let text = fs::read_to_string(path).map_err(|e| {
        ClangError::new(
            1,
            format!("cannot read '{}': {}", path, e),
            "check the file path".to_string(),
        )
    })?;
    if text.starts_with("CLNGB1\n") {
        let payload = text["CLNGB1\n".len()..].trim();
        deobfuscate(payload)
    } else {
        Ok(text)
    }
}
