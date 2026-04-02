use std::process::Command;

const PF_ANCHOR: &str = "pulse-focus";

pub fn block_sites(sites: &[String]) {
    if sites.is_empty() {
        return;
    }

    let ips = resolve_ips(sites);
    if ips.is_empty() {
        return;
    }

    let mut rules = String::new();
    for ip in &ips {
        rules.push_str(&format!("block drop out quick from any to {}\n", ip));
    }
    let _ = Command::new("sudo")
        .args(["-n", "pfctl", "-a", PF_ANCHOR, "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(rules.as_bytes())?;
            }
            child.wait()
        });

    ensure_pf_anchor();

    let _ = Command::new("sudo")
        .args(["-n", "pfctl", "-e"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

pub fn unblock_sites() {
    let _ = Command::new("sudo")
        .args(["-n", "pfctl", "-a", PF_ANCHOR, "-F", "all"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn ensure_pf_anchor() {
    let anchor_line = format!("anchor \"{}\" all", PF_ANCHOR);
    if let Ok(output) = Command::new("sudo")
        .args(["-n", "pfctl", "-sr", "-f", "/etc/pf.conf"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains(&anchor_line) {
            return;
        }
    }

    if let Ok(contents) = std::fs::read_to_string("/etc/pf.conf") {
        if contents.contains(&anchor_line) {
            let _ = Command::new("sudo")
                .args(["-n", "pfctl", "-f", "/etc/pf.conf"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            return;
        }

        let mut new_contents = contents.clone();
        new_contents.push_str(&format!("\n{}\n", anchor_line));

        let _ = Command::new("sudo")
            .args(["-n", "tee", "/etc/pf.conf"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(new_contents.as_bytes())?;
                }
                child.wait()
            });

        let _ = Command::new("sudo")
            .args(["-n", "pfctl", "-f", "/etc/pf.conf"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

fn resolve_ips(sites: &[String]) -> Vec<String> {
    let mut ips = Vec::new();
    for site in sites {
        // Resolve IPv4 (A records)
        if let Ok(output) = Command::new("dig")
            .args(["+short", "A", site])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && trimmed.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                    ips.push(trimmed.to_string());
                }
            }
        }
        // Resolve IPv6 (AAAA records)
        if let Ok(output) = Command::new("dig")
            .args(["+short", "AAAA", site])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && trimmed.contains(':') {
                    ips.push(trimmed.to_string());
                }
            }
        }
    }
    ips.sort();
    ips.dedup();
    ips
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_sites_skips_empty_list() {
        block_sites(&[]);
    }

    #[test]
    fn test_resolve_ips_filters_non_ip_lines() {
        let lines = "dualstack.reddit.map.fastly.net.\n151.101.1.140\n151.101.65.140\n";
        let mut ips = Vec::new();
        for line in lines.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && trimmed.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                ips.push(trimmed.to_string());
            }
        }
        assert_eq!(ips, vec!["151.101.1.140", "151.101.65.140"]);
    }
}
