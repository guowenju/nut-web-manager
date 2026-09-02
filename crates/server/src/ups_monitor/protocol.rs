use std::{collections::BTreeMap, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_LINES: usize = 10_000;

#[derive(Clone, Debug, serde::Serialize)]
pub struct DiscoveredUps {
    pub name: String,
    pub description: Option<String>,
}

pub async fn list_ups(address: &str, port: u16) -> Result<Vec<DiscoveredUps>, String> {
    let lines = request(address, port, "LIST UPS\n", "END LIST UPS").await?;
    let mut devices = Vec::new();
    for line in lines {
        let Some(rest) = line.strip_prefix("UPS ") else {
            continue;
        };
        let Some((name, encoded)) = rest.split_once(' ') else {
            continue;
        };
        devices.push(DiscoveredUps {
            name: name.to_owned(),
            description: decode_value(encoded).filter(|value| value != "Unavailable"),
        });
    }
    if devices.is_empty() {
        Err("upsd returned no UPS devices".into())
    } else {
        Ok(devices)
    }
}

pub async fn list_variables(
    address: &str,
    port: u16,
    ups_name: &str,
) -> Result<BTreeMap<String, String>, String> {
    validate_ups_name(ups_name)?;
    let command = format!("LIST VAR {ups_name}\n");
    let end = format!("END LIST VAR {ups_name}");
    let lines = request(address, port, &command, &end).await?;
    let prefix = format!("VAR {ups_name} ");
    let mut variables = BTreeMap::new();
    for line in lines {
        let Some(rest) = line.strip_prefix(&prefix) else {
            continue;
        };
        let Some((key, encoded)) = rest.split_once(' ') else {
            continue;
        };
        if let Some(value) = decode_value(encoded) {
            variables.insert(key.to_owned(), value);
        }
    }
    if variables.is_empty() {
        Err("upsd returned no UPS variables".into())
    } else {
        Ok(variables)
    }
}

async fn request(
    address: &str,
    port: u16,
    command: &str,
    expected_end: &str,
) -> Result<Vec<String>, String> {
    let destination = if address.contains(':') && !address.starts_with('[') {
        format!("[{address}]:{port}")
    } else {
        format!("{address}:{port}")
    };
    timeout(
        NETWORK_TIMEOUT,
        request_inner(&destination, command, expected_end),
    )
    .await
    .map_err(|_| "NUT request timed out".to_owned())?
}

async fn request_inner(
    destination: &str,
    command: &str,
    expected_end: &str,
) -> Result<Vec<String>, String> {
    let mut stream = TcpStream::connect(destination)
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(command.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    let mut stream = BufReader::new(stream.take((MAX_RESPONSE_BYTES + 1) as u64));

    let mut lines = Vec::new();
    let mut response_bytes = 0_usize;
    loop {
        let mut response = String::new();
        let bytes = stream
            .read_line(&mut response)
            .await
            .map_err(|error| error.to_string())?;
        if bytes == 0 {
            return Err("upsd closed the connection before completing the response".into());
        }
        response_bytes = response_bytes.saturating_add(bytes);
        if response_bytes > MAX_RESPONSE_BYTES || lines.len() >= MAX_RESPONSE_LINES {
            return Err("upsd response exceeded the allowed size".into());
        }
        let line = response.trim().to_owned();
        if line == expected_end {
            return Ok(lines);
        }
        if line.starts_with("ERR ") {
            return Err(format!("upsd returned: {line}"));
        }
        lines.push(line);
    }
}

fn validate_ups_name(value: &str) -> Result<(), String> {
    if !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        Ok(())
    } else {
        Err("invalid UPS name returned by server".into())
    }
}

fn decode_value(value: &str) -> Option<String> {
    let value = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut decoded = String::with_capacity(value.len());
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            decoded.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            decoded.push(character);
        }
    }
    if escaped {
        decoded.push('\\');
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    async fn server(response: &'static str) -> (String, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        (address.ip().to_string(), address.port())
    }

    #[tokio::test]
    async fn discovers_multiple_devices() {
        let (address, port) = server(
            "BEGIN LIST UPS\nUPS ups0 \"UGREEN US7000\"\nUPS backup \"Lab \\\"UPS\\\"\"\nEND LIST UPS\n",
        )
        .await;
        let devices = list_ups(&address, port).await.unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[1].description.as_deref(), Some("Lab \"UPS\""));
    }

    #[tokio::test]
    async fn parses_ugreen_variables_and_unknown_fields() {
        let (address, port) = server(
            "BEGIN LIST VAR ups0\nVAR ups0 battery.charge \"100\"\nVAR ups0 battery.runtime \"65535\"\nVAR ups0 device.mfr \"UGREEN\"\nVAR ups0 outlet.1.desc \"PowerShare Outlet 1\"\nEND LIST VAR ups0\n",
        )
        .await;
        let values = list_variables(&address, port, "ups0").await.unwrap();
        assert_eq!(values["device.mfr"], "UGREEN");
        assert_eq!(values["outlet.1.desc"], "PowerShare Outlet 1");
    }

    #[tokio::test]
    async fn reports_incomplete_response() {
        let (address, port) = server("BEGIN LIST UPS\nUPS ups0 \"UPS\"\n").await;
        let error = list_ups(&address, port).await.unwrap_err();
        assert!(
            error.contains("closed") || error.contains("reset"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn rejects_oversized_responses() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut command = [0_u8; 64];
            let _ = socket.read(&mut command).await;
            let response = vec![b'x'; MAX_RESPONSE_BYTES + 1];
            let _ = socket.write_all(&response).await;
        });

        let error = list_ups(&address.ip().to_string(), address.port())
            .await
            .unwrap_err();
        assert!(error.contains("allowed size"), "{error}");
    }
}
