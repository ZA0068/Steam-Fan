use std::process::Command;

/// Open a folder or a `steam://` link with the desktop's default app.
pub fn open(target: &str) {
    let _ = Command::new("xdg-open").arg(target).spawn();
}

/// A folder picker from the desktop (KDE or GNOME), if one is installed.
pub fn pick_folder(title: &str, start: &str) -> Option<String> {
    let tries: [(&str, Vec<&str>); 2] = [
        ("kdialog", vec!["--getexistingdirectory", start, "--title", title]),
        ("zenity", vec!["--file-selection", "--directory", "--title", title, "--filename", start]),
    ];
    for (prog, args) in tries {
        if let Ok(out) = Command::new(prog).args(&args).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
            return None; // the picker ran and was cancelled
        }
    }
    None
}

/// Split a command line into words; "double quotes" keep spaces together.
pub fn split_words(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn join_words(w: &[String]) -> String {
    w.iter().map(|a| if a.contains(' ') || a.is_empty() { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn words() {
        let w = split_words(r#"retroarch -L "/my cores/snes.so" {rom}"#);
        assert_eq!(w, ["retroarch", "-L", "/my cores/snes.so", "{rom}"]);
        assert_eq!(split_words(&join_words(&w)), w);
    }
}
