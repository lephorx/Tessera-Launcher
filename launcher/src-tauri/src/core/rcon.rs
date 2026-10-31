//! Minecraft's RCON: how the launcher types commands into a server it runs. The server keeps
//! running when the launcher quits, so the launcher can't keep its console (stdin); RCON
//! works from any launcher session. Packets are `length, id, type, body\0\0`, little endian.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const LOGIN: i32 = 3;
const COMMAND: i32 = 2;

async fn write(stream: &mut TcpStream, id: i32, kind: i32, body: &str) -> Result<()> {
    let mut p = Vec::with_capacity(14 + body.len());
    p.extend_from_slice(&((10 + body.len()) as i32).to_le_bytes());
    p.extend_from_slice(&id.to_le_bytes());
    p.extend_from_slice(&kind.to_le_bytes());
    p.extend_from_slice(body.as_bytes());
    p.extend_from_slice(&[0, 0]);
    stream.write_all(&p).await?;
    Ok(())
}

async fn read(stream: &mut TcpStream) -> Result<(i32, String)> {
    let len = stream.read_i32_le().await?;
    if !(10..=1 << 20).contains(&len) {
        bail!("RCON sent an odd packet");
    }
    let mut buf = vec![0; len as usize];
    stream.read_exact(&mut buf).await?;
    let id = i32::from_le_bytes(buf[0..4].try_into()?);
    let body = String::from_utf8_lossy(&buf[8..buf.len() - 2]).into_owned();
    Ok((id, body))
}

/// Runs one command and returns what the server answered.
pub async fn command(port: u16, password: &str, line: &str) -> Result<String> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.context("The server isn't accepting commands yet")?;
        write(&mut s, 1, LOGIN, password).await?;
        let (id, _) = read(&mut s).await?;
        if id == -1 {
            bail!("The server refused the launcher's password");
        }
        write(&mut s, 2, COMMAND, line).await?;
        let (_, body) = read(&mut s).await?;
        Ok(body)
    })
    .await
    .context("The server didn't answer")?
}

/// Whether the server answers on RCON yet (it opens RCON once the world is loaded).
pub async fn ready(port: u16, password: &str) -> bool {
    command(port, password, "list").await.is_ok()
}
