use std::{collections::BTreeMap, time::Duration};

use chrono::{DateTime, Utc};
use nwm_common::Host;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ssh::{SshError, SshManager};

const SCAN_TIMEOUT: Duration = Duration::from_secs(60);
const SCAN_SCRIPT: &str = r#"#!/bin/sh
set -u

if ! dpkg-query -W -f='${Status}' nut-server 2>/dev/null | grep -q '^install ok installed$'; then
    printf '%s\n' 'NWM_SCAN_ERROR=NutServerNotInstalled'
    exit 0
fi

if ! command -v nut-scanner >/dev/null 2>&1; then
    printf '%s\n' 'NWM_SCAN_ERROR=ScannerUnavailable'
    exit 0
fi

available=$(nut-scanner -a 2>&1)
available_code=$?
if [ "$available_code" -ne 0 ]; then
    printf '%s\n' 'NWM_SCAN_ERROR=ScannerUnavailable'
    printf '%s\n' "$available"
    exit 0
fi
if ! printf '%s\n' "$available" | grep -Eiq '(^|[[:space:],])USB([[:space:],]|$)'; then
    printf '%s\n' 'NWM_SCAN_ERROR=UsbScanUnavailable'
    printf '%s\n' "$available"
    exit 0
fi

output=$(nut-scanner -U -P -q 2>&1)
scan_code=$?
if [ "$scan_code" -eq 0 ]; then
    printf '%s\n' 'NWM_SCAN_FORMAT=parsable'
    printf '%s\n' "$output"
    exit 0
fi

if printf '%s\n' "$output" | grep -Eiq '(unknown|unrecognized|invalid).*(option|argument)|illegal option'; then
    output=$(nut-scanner -U -N -q 2>&1)
    scan_code=$?
    if [ "$scan_code" -eq 0 ]; then
        printf '%s\n' 'NWM_SCAN_FORMAT=nut_conf'
        printf '%s\n' "$output"
        exit 0
    fi
fi

printf '%s\n' 'NWM_SCAN_ERROR=UsbScanFailed'
printf '%s\n' "$output"
"#;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanFormat {
    Parsable,
    NutConf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsbScanCandidate {
    pub index: usize,
    pub driver: String,
    pub port: String,
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
    pub bus: Option<String>,
    pub device: Option<String>,
    pub selectors: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsbScanResult {
    pub format: ScanFormat,
    pub scanned_at: DateTime<Utc>,
    pub candidates: Vec<UsbScanCandidate>,
}

#[derive(Debug, Error)]
pub enum UsbScanError {
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error("nut-server is not installed on the Server host")]
    NutServerNotInstalled,
    #[error("nut-scanner is not installed or cannot run")]
    ScannerUnavailable,
    #[error("this nut-scanner build does not provide USB scanning")]
    UsbScanUnavailable,
    #[error("USB scanning failed: {0}")]
    UsbScanFailed(String),
    #[error("nut-scanner returned an unsupported response: {0}")]
    InvalidOutput(String),
}

pub async fn scan(ssh: &SshManager, host: &Host) -> Result<UsbScanResult, UsbScanError> {
    let output = ssh.execute_script(host, SCAN_SCRIPT, SCAN_TIMEOUT).await?;
    parse_scan_output(&output)
}

fn parse_scan_output(output: &str) -> Result<UsbScanResult, UsbScanError> {
    let mut lines = output.lines();
    let marker = lines.next().unwrap_or_default().trim();
    let body = lines.collect::<Vec<_>>().join("\n");
    match marker {
        "NWM_SCAN_ERROR=NutServerNotInstalled" => Err(UsbScanError::NutServerNotInstalled),
        "NWM_SCAN_ERROR=ScannerUnavailable" => Err(UsbScanError::ScannerUnavailable),
        "NWM_SCAN_ERROR=UsbScanUnavailable" => Err(UsbScanError::UsbScanUnavailable),
        "NWM_SCAN_ERROR=UsbScanFailed" => Err(UsbScanError::UsbScanFailed(body.trim().to_owned())),
        "NWM_SCAN_FORMAT=parsable" => Ok(UsbScanResult {
            format: ScanFormat::Parsable,
            scanned_at: Utc::now(),
            candidates: parse_parsable(&body)?,
        }),
        "NWM_SCAN_FORMAT=nut_conf" => Ok(UsbScanResult {
            format: ScanFormat::NutConf,
            scanned_at: Utc::now(),
            candidates: parse_nut_conf(&body)?,
        }),
        _ => Err(UsbScanError::InvalidOutput(truncate(output))),
    }
}

fn parse_parsable(output: &str) -> Result<Vec<UsbScanCandidate>, UsbScanError> {
    output
        .lines()
        // Debian's nut-scanner writes missing optional backend-library warnings
        // into the same captured stream even for a successful `-U` scan. Only
        // `USB:` records belong to the documented parsable result format.
        .filter(|line| {
            line.trim_start()
                .get(..4)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("USB:"))
        })
        .enumerate()
        .map(|(index, line)| {
            let fields = split_fields(line.trim());
            let first = fields
                .first()
                .ok_or_else(|| UsbScanError::InvalidOutput(line.into()))?;
            let (bus_type, first_field) = first
                .split_once(':')
                .ok_or_else(|| UsbScanError::InvalidOutput(line.into()))?;
            if !bus_type.eq_ignore_ascii_case("USB") {
                return Err(UsbScanError::InvalidOutput(format!(
                    "unexpected scan bus: {bus_type}"
                )));
            }
            let mut values = BTreeMap::new();
            for field in
                std::iter::once(first_field).chain(fields.iter().skip(1).map(String::as_str))
            {
                if let Some((key, value)) = field.split_once('=') {
                    values.insert(key.trim().to_ascii_lowercase(), unquote(value.trim()));
                }
            }
            candidate(index, values)
        })
        .collect()
}

fn parse_nut_conf(output: &str) -> Result<Vec<UsbScanCandidate>, UsbScanError> {
    let mut devices = Vec::new();
    let mut current: Option<BTreeMap<String, String>> = None;
    for raw_line in output.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(values) = current.take() {
                devices.push(candidate(devices.len(), values)?);
            }
            current = Some(BTreeMap::new());
        } else if let (Some(values), Some((key, value))) = (current.as_mut(), line.split_once('='))
        {
            values.insert(key.trim().to_ascii_lowercase(), unquote(value.trim()));
        }
    }
    if let Some(values) = current {
        devices.push(candidate(devices.len(), values)?);
    }
    Ok(devices)
}

fn candidate(
    index: usize,
    mut values: BTreeMap<String, String>,
) -> Result<UsbScanCandidate, UsbScanError> {
    let driver = values
        .remove("driver")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| UsbScanError::InvalidOutput("scan candidate has no driver".into()))?;
    let port = values
        .remove("port")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| UsbScanError::InvalidOutput("scan candidate has no port".into()))?;
    Ok(UsbScanCandidate {
        index,
        driver,
        port,
        vendor: values.get("vendor").cloned(),
        product: values.get("product").cloned(),
        serial: values.get("serial").cloned(),
        vendor_id: values.get("vendorid").cloned(),
        product_id: values.get("productid").cloned(),
        bus: values.get("bus").cloned(),
        device: values.get("device").cloned(),
        selectors: values,
    })
}

fn split_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in line.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                current.push(character);
            }
            ',' if !quoted => {
                fields.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
    }
    fields.push(current);
    fields
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_owned()
}

fn truncate(value: &str) -> String {
    value.chars().take(1000).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_parsable_shape() {
        let result = parse_scan_output(concat!(
            "NWM_SCAN_FORMAT=parsable\n",
            "USB:driver=\"usbhid-ups\",port=\"auto\",vendorid=\"051d\",productid=\"0002\",vendor=\"APC\",product=\"Back-UPS\",serial=\"ABC\",bus=\"001\",device=\"002\"\n"
        )).unwrap();
        assert_eq!(result.candidates.len(), 1);
        let device = &result.candidates[0];
        assert_eq!(device.driver, "usbhid-ups");
        assert_eq!(device.vendor_id.as_deref(), Some("051d"));
        assert_eq!(device.product.as_deref(), Some("Back-UPS"));
    }

    #[test]
    fn parses_nut_conf_fallback_and_multiple_devices() {
        let result = parse_scan_output(concat!(
            "NWM_SCAN_FORMAT=nut_conf\n",
            "[nutdev1]\n driver = \"usbhid-ups\"\n port = \"auto\"\n vendorid = \"051d\"\n\n",
            "[nutdev2]\n driver = \"blazer_usb\"\n port = \"auto\"\n product = \"UPS, Model\"\n"
        ))
        .unwrap();
        assert_eq!(result.candidates.len(), 2);
        assert_eq!(result.candidates[1].product.as_deref(), Some("UPS, Model"));
    }

    #[test]
    fn empty_success_is_not_an_error() {
        let result = parse_scan_output("NWM_SCAN_FORMAT=parsable\n").unwrap();
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn preserves_commas_inside_parsable_values() {
        let devices =
            parse_parsable("USB:driver=\"usbhid-ups\",port=\"auto\",product=\"UPS, Model\"")
                .unwrap();
        assert_eq!(devices[0].product.as_deref(), Some("UPS, Model"));
    }

    #[test]
    fn ignores_debian_optional_backend_warnings_during_usb_scan() {
        let result = parse_scan_output(concat!(
            "NWM_SCAN_FORMAT=parsable\n",
            "Cannot load SNMP library (libnetsnmp.so.40) : file not found. SNMP search disabled.\n",
            "Cannot load XML library (libneon.so.27) : file not found. XML search disabled.\n",
            "Cannot load AVAHI library (libavahi-client.so.3) : file not found. AVAHI search disabled.\n",
            "Cannot load IPMI library (libfreeipmi.so.17) : file not found. IPMI search disabled.\n",
            "USB:driver=\"usbhid-ups\",port=\"auto\",vendorid=\"0463\",productid=\"FFFF\",product=\"SANTAK TG-BOX\",serial=\"Blank\",vendor=\"EATON\",bus=\"003\",device=\"002\",busport=\"001\"\n",
        ))
        .unwrap();

        assert_eq!(result.candidates.len(), 1);
        let device = &result.candidates[0];
        assert_eq!(device.driver, "usbhid-ups");
        assert_eq!(device.vendor.as_deref(), Some("EATON"));
        assert_eq!(device.product.as_deref(), Some("SANTAK TG-BOX"));
        assert_eq!(device.vendor_id.as_deref(), Some("0463"));
        assert_eq!(device.product_id.as_deref(), Some("FFFF"));
        assert_eq!(
            device.selectors.get("busport").map(String::as_str),
            Some("001")
        );
    }
}
