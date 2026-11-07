//! Foreground process inspection without shell integration (§3.1, `at-procinfo`).

/// Short process name.
pub fn name(pid: i32) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let mut buf = [0u8; 256];
        let n = unsafe {
            libc::proc_name(pid, buf.as_mut_ptr() as *mut libc::c_void, buf.len() as u32)
        };
        if n <= 0 {
            return None;
        }
        Some(String::from_utf8_lossy(&buf[..n as usize]).into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|s| s.trim().to_string())
    }
}

/// Current working directory of a process.
pub fn cwd(pid: i32) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        const PROC_PIDVNODEPATHINFO: libc::c_int = 9;
        // struct proc_vnodepathinfo { vnode_info_path cdir; vnode_info_path rdir; }
        // vnode_info_path { vnode_info (152 bytes); char path[MAXPATHLEN=1024]; }
        const VI_SIZE: usize = 152;
        const PATH_MAX: usize = 1024;
        let mut buf = vec![0u8; 2 * (VI_SIZE + PATH_MAX)];
        let n = unsafe {
            libc::proc_pidinfo(
                pid,
                PROC_PIDVNODEPATHINFO,
                0,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len() as libc::c_int,
            )
        };
        if n <= 0 {
            return None;
        }
        let path = &buf[VI_SIZE..VI_SIZE + PATH_MAX];
        let end = path.iter().position(|b| *b == 0).unwrap_or(PATH_MAX);
        let s = String::from_utf8_lossy(&path[..end]).into_owned();
        (!s.is_empty()).then_some(s)
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::read_link(format!("/proc/{pid}/cwd"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }
}

/// Command-line arguments (argv) of a process.
pub fn args(pid: i32) -> Option<Vec<String>> {
    #[cfg(target_os = "macos")]
    {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut size: libc::size_t = 0;
        unsafe {
            if libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            ) != 0
            {
                return None;
            }
        }
        let mut buf = vec![0u8; size];
        unsafe {
            if libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr() as *mut libc::c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            ) != 0
            {
                return None;
            }
        }
        buf.truncate(size);
        if buf.len() < 4 {
            return None;
        }
        let argc = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        let mut i = 4;
        // Skip exec path and the NUL padding after it.
        while i < buf.len() && buf[i] != 0 {
            i += 1;
        }
        while i < buf.len() && buf[i] == 0 {
            i += 1;
        }
        let mut out = Vec::with_capacity(argc);
        while out.len() < argc && i < buf.len() {
            let start = i;
            while i < buf.len() && buf[i] != 0 {
                i += 1;
            }
            out.push(String::from_utf8_lossy(&buf[start..i]).into_owned());
            i += 1;
        }
        Some(out)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        Some(
            raw.split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect(),
        )
    }
}

/// Destination host from ssh-like arguments (`ssh -p 22 user@host cmd` → `host`).
pub fn remote_host(args: &[String]) -> Option<String> {
    let prog = args.first().map(|a| {
        std::path::Path::new(a)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    })?;
    let rest = &args[1..];
    match prog.as_str() {
        "ssh" | "mosh-client" | "et" | "telnet" | "nc" | "mosh" => {
            const WITH_ARG: &str = "BbcDEeFIiJLlmOoPpQRSWw";
            let mut skip = false;
            for a in rest {
                if skip {
                    skip = false;
                    continue;
                }
                if let Some(opt) = a.strip_prefix('-') {
                    if opt.len() == 1 && WITH_ARG.contains(opt) {
                        skip = true;
                    }
                    continue;
                }
                if prog == "mosh-client" {
                    // mosh-client IP PORT
                    return Some(a.clone());
                }
                let host = a.rsplit('@').next().unwrap_or(a);
                let host = host.split(':').next().unwrap_or(host);
                return Some(host.to_string());
            }
            None
        }
        "kubectl" | "docker" | "podman" => {
            let i = rest
                .iter()
                .position(|a| a == "exec" || a == "attach" || a == "run")?;
            rest[i + 1..].iter().find(|a| !a.starts_with('-')).cloned()
        }
        _ => None,
    }
}

/// Whether `name`/`args` describe a remote session given the configured program list.
pub fn is_remote(name: &str, args: Option<&[String]>, programs: &[String]) -> bool {
    if !programs.iter().any(|p| p == name) {
        return false;
    }
    match name {
        "kubectl" | "docker" | "podman" => args
            .map(|a| a.iter().any(|x| x == "exec" || x == "attach"))
            .unwrap_or(false),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }

    #[test]
    fn hosts() {
        assert_eq!(
            remote_host(&v("ssh -p 2222 -o Foo=bar admin@srv-01 uptime")).as_deref(),
            Some("srv-01")
        );
        assert_eq!(remote_host(&v("/usr/bin/ssh db")).as_deref(), Some("db"));
        assert_eq!(
            remote_host(&v("kubectl exec -it pod-1 -- sh")).as_deref(),
            Some("pod-1")
        );
        assert_eq!(remote_host(&v("vim file")), None);
    }

    #[test]
    fn own_process() {
        let pid = std::process::id() as i32;
        assert!(name(pid).is_some());
        let c = cwd(pid).unwrap();
        assert_eq!(
            std::path::Path::new(&c).canonicalize().ok(),
            std::env::current_dir().unwrap().canonicalize().ok()
        );
        let a = args(pid).unwrap();
        assert!(!a.is_empty());
    }

    #[test]
    fn remote_rules() {
        let progs: Vec<String> = ["ssh", "kubectl"].map(String::from).to_vec();
        assert!(is_remote("ssh", None, &progs));
        assert!(!is_remote("kubectl", Some(&v("kubectl get pods")), &progs));
        assert!(is_remote(
            "kubectl",
            Some(&v("kubectl exec -it x sh")),
            &progs
        ));
        assert!(!is_remote("vim", None, &progs));
    }
}

/// Snapshot the process tree before signalling its root (parents precede children).
pub fn descendants(root: i32) -> Vec<i32> {
    let Ok(output) = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid="]).output() else { return Vec::new(); };
    let rows: Vec<(i32, i32)> = String::from_utf8_lossy(&output.stdout)
        .lines().filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        }).collect();
    let mut tree = vec![root];
    let mut index = 0;
    while index < tree.len() {
        let parent = tree[index];
        for &(pid, ppid) in &rows {
            if ppid == parent && !tree.contains(&pid) { tree.push(pid); }
        }
        index += 1;
    }
    // Job-control groups can outlive their shell while retaining its session ID.
    for &(pid, _) in &rows {
        if pid != root && unsafe { libc::getsid(pid) } == root && !tree.contains(&pid) {
            tree.push(pid);
        }
    }
    tree.remove(0);
    tree
}
