//! Recherche des outils externes (untrunc, ffmpeg, ffprobe, PhotoRec) et appels à ffprobe / ffmpeg.

use crate::platform::sans_console;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn exe(nom: &str) -> String {
    if cfg!(windows) && !nom.to_lowercase().ends_with(".exe") {
        format!("{nom}.exe")
    } else {
        nom.to_string()
    }
}

/// Dossier de l'exécutable d'Ataecina.
pub fn dossier_app() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

/// Cherche un outil dans les dossiers donnés, dans le sous-dossier `outils` d'Ataecina, puis dans le PATH.
pub fn trouver(nom: &str, dossiers: &[PathBuf]) -> Option<PathBuf> {
    let e = exe(nom);
    let app = dossier_app();
    let mut cands: Vec<PathBuf> = dossiers.to_vec();
    for sous in ["", "outils", "tools"] {
        let base = if sous.is_empty() { app.clone() } else { app.join(sous) };
        cands.push(base.clone());
        // un niveau de sous-dossiers : outils/ffmpeg/bin, outils/untrunc, outils/testdisk-7.2 ...
        if let Ok(rd) = std::fs::read_dir(&base) {
            for d in rd.flatten() {
                let p = d.path();
                if p.is_dir() {
                    cands.push(p.join("bin"));
                    cands.push(p.clone());
                    if let Ok(rd2) = std::fs::read_dir(&p) {
                        for d2 in rd2.flatten() {
                            let p2 = d2.path();
                            if p2.is_dir() {
                                cands.push(p2.join("bin"));
                                cands.push(p2);
                            }
                        }
                    }
                }
            }
        }
    }
    for d in cands {
        if d.as_os_str().is_empty() {
            continue;
        }
        let p = d.join(&e);
        if crate::existe_fichier(&p) {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(&e)).find(|p| crate::existe_fichier(p))
}

/// PhotoRec : chemin configuré, ou photorec_win.exe / photorec trouvé automatiquement.
pub fn trouver_photorec(config: &str) -> Option<PathBuf> {
    if !config.is_empty() && crate::existe_fichier(Path::new(config)) {
        return Some(PathBuf::from(config));
    }
    for n in ["photorec_win", "photorec"] {
        if let Some(p) = trouver(n, &[]) {
            return Some(p);
        }
    }
    None
}

/// Lance une commande avec arrêt possible ; renvoie (code de sortie réussi, stdout, stderr).
pub fn executer(cmd: &mut Command, stop: &AtomicBool, delai: Option<Duration>) -> Option<(bool, Vec<u8>, Vec<u8>)> {
    use std::io::Read;
    use std::process::Stdio;
    sans_console(cmd);
    let mut ch = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let mut out = ch.stdout.take()?;
    let mut err = ch.stderr.take()?;
    let t1 = std::thread::spawn(move || {
        let mut v = vec![];
        let _ = out.read_to_end(&mut v);
        v
    });
    let t2 = std::thread::spawn(move || {
        let mut v = vec![];
        let _ = err.read_to_end(&mut v);
        v
    });
    let debut = std::time::Instant::now();
    let ok = loop {
        match ch.try_wait() {
            Ok(Some(s)) => break s.success(),
            Ok(None) => {}
            Err(_) => break false,
        }
        if stop.load(Ordering::Relaxed) || delai.is_some_and(|d| debut.elapsed() > d) {
            let _ = ch.kill();
            let _ = ch.wait();
            break false;
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    Some((ok, t1.join().unwrap_or_default(), t2.join().unwrap_or_default()))
}

/// Durée lue par ffprobe, 0 si le fichier n'est pas lisible.
pub fn duree(ffprobe: &Path, f: &Path, stop: &AtomicBool) -> f64 {
    let mut c = Command::new(ffprobe);
    c.args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"]).arg(f);
    let Some((_, out, _)) = executer(&mut c, stop, Some(Duration::from_secs(60))) else { return 0.0 };
    String::from_utf8_lossy(&out).lines().next().and_then(|s| s.trim().parse::<f64>().ok()).filter(|v| *v > 0.0).unwrap_or(0.0)
}

/// Décode entièrement le fichier et compte les lignes d'erreur de ffmpeg.
pub fn erreurs(ffmpeg: &Path, f: &Path, stop: &AtomicBool) -> usize {
    let mut c = Command::new(ffmpeg);
    c.args(["-nostdin", "-v", "error", "-i"]).arg(f).args(["-f", "null", "-"]);
    let Some((_, _, err)) = executer(&mut c, stop, None) else { return 1 };
    String::from_utf8_lossy(&err).lines().filter(|l| !l.trim().is_empty()).count()
}

/// Image JPEG tirée d'une vidéo (vignette), ou vide.
pub fn vignette(ffmpeg: &Path, f: &Path) -> Vec<u8> {
    let stop = AtomicBool::new(false);
    for ss in [Some("0.6"), None] {
        let mut c = Command::new(ffmpeg);
        c.args(["-nostdin", "-v", "error"]);
        if let Some(s) = ss {
            c.args(["-ss", s]);
        }
        c.arg("-i").arg(f).args(["-frames:v", "1", "-vf", "scale=480:-2", "-f", "image2pipe", "-vcodec", "mjpeg", "-q:v", "4", "-"]);
        if let Some((_, out, _)) = executer(&mut c, &stop, Some(Duration::from_secs(20))) {
            if !out.is_empty() {
                return out;
            }
        }
    }
    vec![]
}
