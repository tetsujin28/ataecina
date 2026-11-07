// Masque la console sous Windows en version release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Ataecina : application Tauri. L'interface parle au moteur par une seule commande `api`,
//! avec les mêmes chemins et les mêmes réponses JSON que la version précédente.

use ataecina_core::i18n::l;
use ataecina_core::photorec::{self, RecConfig, ERR_ADMIN};
use ataecina_core::{analyse, existe_fichier, platform, repair, tools};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

const VERSION: &str = "Bêta 0.11.1";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct Config {
    references: Vec<String>,
    files: Vec<String>,
    sortie: String,
    untrunc: String,
    #[serde(rename = "ffmpegDir")]
    ffmpeg_dir: String,
    verif: bool,
    #[serde(rename = "ignoreFtyp")]
    ignore_ftyp: bool,
    #[serde(rename = "lastDir")]
    last_dir: String,
    rec: RecConfig,
    lang: String,
}

#[derive(Default)]
struct App {
    cfg: Arc<Mutex<Config>>,
    cfg_path: Arc<Mutex<PathBuf>>,
    rep: Arc<Mutex<repair::Etat>>,
    rep_stop: Arc<AtomicBool>,
    rec: Arc<Mutex<photorec::Etat>>,
    rec_stop: Arc<AtomicBool>,
    fs: Arc<Mutex<analyse::Etat>>,
    fs_stop: Arc<AtomicBool>,
    vignettes: Mutex<HashMap<String, String>>,
}

fn sauver_config(cfg: &Mutex<Config>, path: &Mutex<PathBuf>) {
    let p = path.lock().unwrap().clone();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_string_pretty(&*cfg.lock().unwrap()) {
        let _ = std::fs::write(p, s);
    }
}

impl App {
    fn sauver(&self) {
        sauver_config(&self.cfg, &self.cfg_path);
    }

    fn outils(&self) -> (Option<PathBuf>, Option<PathBuf>, Option<PathBuf>) {
        let c = self.cfg.lock().unwrap();
        let untrunc = if existe_fichier(Path::new(&c.untrunc)) { Some(PathBuf::from(&c.untrunc)) } else { tools::trouver("untrunc", &[]) };
        let d = PathBuf::from(&c.ffmpeg_dir);
        let dossiers = if c.ffmpeg_dir.is_empty() { vec![] } else { vec![d.clone(), d.join("bin")] };
        (untrunc, tools::trouver("ffprobe", &dossiers), tools::trouver("ffmpeg", &dossiers))
    }

    fn etat(&self) -> Value {
        let (u, fp, fm) = self.outils();
        let c = self.cfg.lock().unwrap().clone();
        let s = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        json!({
            "config": c,
            "references": infos(&c.references),
            "files": infos(&c.files),
            "tools": {"untrunc": s(&u), "ffprobe": s(&fp), "ffmpeg": s(&fm)},
            "windows": cfg!(windows),
            "version": VERSION,
            "photorec": tools::trouver_photorec(&c.rec.photorec).map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            "admin": platform::est_admin(),
        })
    }
}

fn infos(l: &[String]) -> Vec<Value> {
    l.iter()
        .map(|p| {
            let pb = Path::new(p);
            let m = pb.metadata().ok().filter(|m| m.is_file());
            let base = pb.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            json!({
                "path": p,
                "name": pb.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                "dir": pb.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default(),
                "size": m.as_ref().map(|m| m.len()).unwrap_or(0),
                "exists": m.is_some(),
                "ftyp": base.ends_with("_ftyp"),
            })
        })
        .collect()
}

fn uniques(l: Vec<String>) -> Vec<String> {
    let mut vus = HashSet::new();
    l.into_iter().filter(|p| !p.is_empty() && vus.insert(if cfg!(windows) { p.to_lowercase() } else { p.clone() })).collect()
}

fn videos_du_dossier(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && matches!(p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref(), Some("mov") | Some("mp4")))
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn ecoule(d: Option<Instant>, f: Option<Instant>, running: bool) -> f64 {
    match (d, f) {
        (Some(d), Some(f)) if !running => (f - d).as_secs_f64(),
        (Some(d), _) => d.elapsed().as_secs_f64(),
        _ => 0.0,
    }
}

fn depuis(logs: &[String], since: usize) -> Vec<String> {
    logs[since.min(logs.len())..].to_vec()
}

fn erreur_admin(e: String) -> String {
    if e == ERR_ADMIN {
        l("Lire une carte directement demande les droits administrateur.")
    } else {
        e
    }
}

/// Analyse le chemin « /api/xxx?a=1&b=2 ».
fn decouper(path: &str) -> (String, HashMap<String, String>) {
    let (p, q) = path.split_once('?').unwrap_or((path, ""));
    let params = q
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), decoder(v)))
        .collect();
    (p.to_string(), params)
}

fn decoder(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[tauri::command]
async fn api(app: AppHandle, st: State<'_, App>, path: String, body: Option<Value>) -> Result<Value, String> {
    traiter(&st, Some(&app), path, body).await
}

/// Route une requête de l'interface. `app` est absent en mode serveur de test.
async fn traiter(st: &App, app: Option<&AppHandle>, path: String, body: Option<Value>) -> Result<Value, String> {
    let (route, q) = decouper(&path);
    let body = body.unwrap_or(Value::Null);
    let since: usize = q.get("since").and_then(|s| s.parse().ok()).unwrap_or(0);
    match route.as_str() {
        "/api/state" => Ok(st.etat()),
        "/api/config" => {
            let mut c: Config = serde_json::from_value(body).map_err(|_| l("Requête invalide"))?;
            c.references = uniques(c.references);
            c.files = uniques(c.files);
            ataecina_core::i18n::set_lang(&c.lang);
            *st.cfg.lock().unwrap() = c;
            st.sauver();
            Ok(st.etat())
        }
        "/api/add" => {
            let target = body["target"].as_str().unwrap_or("files").to_string();
            let mut nouveaux: Vec<String> = body["paths"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            for d in body["folders"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
                nouveaux.extend(videos_du_dossier(Path::new(d)));
            }
            let ajoutes = {
                let mut c = st.cfg.lock().unwrap();
                let liste = if target == "references" { &mut c.references } else { &mut c.files };
                let avant = liste.len();
                let mut l2 = liste.clone();
                l2.extend(nouveaux);
                *liste = uniques(l2);
                let n = liste.len() - avant;
                if let Some(d) = body["lastDir"].as_str().filter(|d| !d.is_empty()) {
                    c.last_dir = d.to_string();
                }
                n
            };
            st.sauver();
            let mut e = st.etat();
            e["added"] = json!(ajoutes);
            Ok(e)
        }
        "/api/pick" => match app {
            Some(a) => choisir(a, st, &body).await,
            None => Ok(Value::Null),
        },

        // ----- réparation -----
        "/api/start" => demarrer_reparation(st),
        "/api/stop" => {
            st.rep_stop.store(true, Ordering::Relaxed);
            st.rep.lock().unwrap().log(l("Arrêt demandé, fin de l'opération en cours..."));
            Ok(json!({"ok": true}))
        }
        "/api/job" => {
            let e = st.rep.lock().unwrap();
            let sortie = st.cfg.lock().unwrap().sortie.clone();
            Ok(json!({
                "running": e.running, "fait": e.fait, "total": e.total, "courant": e.courant,
                "elapsed": ecoule(e.debut, e.fin, e.running), "logs": depuis(&e.logs, since), "logCount": e.logs.len(),
                "results": e.results, "started": e.debut.is_some(), "sortie": sortie,
            }))
        }
        "/api/thumb" => {
            let p = q.get("path").cloned().unwrap_or_default();
            let autorise = st.rep.lock().unwrap().results.iter().any(|r| r.sortie == p);
            if !autorise {
                return Err("not found".into());
            }
            if let Some(v) = st.vignettes.lock().unwrap().get(&p) {
                return Ok(json!({"data": v}));
            }
            let (_, _, ffmpeg) = st.outils();
            let ffmpeg = ffmpeg.ok_or("ffmpeg")?;
            let pp = p.clone();
            let img = tauri::async_runtime::spawn_blocking(move || tools::vignette(&ffmpeg, Path::new(&pp))).await.map_err(|e| e.to_string())?;
            let data = if img.is_empty() { String::new() } else { format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(img)) };
            st.vignettes.lock().unwrap().insert(p, data.clone());
            Ok(json!({"data": data}))
        }
        "/api/open" => {
            let p = PathBuf::from(body["path"].as_str().unwrap_or(""));
            let dossier = if p.is_file() { p.parent().map(|x| x.to_path_buf()).unwrap_or(p) } else { p };
            let _ = std::fs::create_dir_all(&dossier);
            platform::ouvrir_dossier(&dossier);
            Ok(json!({"ok": true}))
        }
        "/api/url" => {
            let u = body["url"].as_str().unwrap_or("");
            if !(u.starts_with("https://") || u.starts_with("http://")) {
                return Err("url".into());
            }
            if let Some(a) = app {
                use tauri_plugin_opener::OpenerExt;
                a.opener().open_url(u, None::<&str>).map_err(|e| e.to_string())?;
            }
            Ok(json!({"ok": true}))
        }
        "/api/ping" | "/api/bye" => Ok(json!({"ok": true})),
        "/api/quit" => {
            st.rep_stop.store(true, Ordering::Relaxed);
            st.rec_stop.store(true, Ordering::Relaxed);
            st.fs_stop.store(true, Ordering::Relaxed);
            st.sauver();
            quitter(app, 400);
            Ok(json!({"ok": true}))
        }
        "/api/elevate" => {
            st.sauver();
            platform::relancer_admin().map_err(|e| format!("{}{e}", l("Relance en administrateur refusée ou impossible : ")))?;
            quitter(app, 600);
            Ok(json!({"ok": true}))
        }
        "/api/drives" => Ok(json!({"drives": platform::lister_lecteurs(), "admin": platform::est_admin()})),

        // ----- récupération PhotoRec -----
        "/api/recover/start" => demarrer_photorec(st),
        "/api/recover/stop" => {
            st.rec_stop.store(true, Ordering::Relaxed);
            st.rec.lock().unwrap().log(l("Arrêt demandé..."));
            Ok(json!({"ok": true}))
        }
        "/api/recover/state" => {
            let e = st.rec.lock().unwrap();
            Ok(json!({
                "running": e.running, "phase": e.phase, "started": e.debut.is_some(), "elapsed": ecoule(e.debut, e.fin, e.running),
                "logs": depuis(&e.logs, since), "logCount": e.logs.len(),
                "imgDone": e.img.fait, "imgTotal": e.img.total, "imgBad": e.img.bad, "imgPath": e.img_path, "imgPass": e.img.passe,
                "session": e.session, "found": e.trouves, "foundSize": e.total_size, "allFiles": e.all_files,
                "error": e.erreur, "sent": e.envoye, "imageOnly": e.image_seul,
            }))
        }
        "/api/recover/send" => {
            let n = envoyer_trouves(st);
            st.sauver();
            let mut e = st.etat();
            e["added"] = json!(n);
            Ok(e)
        }

        // ----- lecture de la carte -----
        "/api/fs/start" => demarrer_analyse(st),
        "/api/fs/copy" => demarrer_copie(st, &body),
        "/api/fs/restore" => {
            let vol = body["vol"].as_u64().unwrap_or(0) as usize;
            let rc = st.cfg.lock().unwrap().rec.clone();
            let (cible, dev) = if rc.source_kind != "image" { (PathBuf::from(platform::chemin_peripherique(&rc.drive)), true) } else { (PathBuf::from(&rc.image), false) };
            let fs = st.fs.clone();
            let msg = tauri::async_runtime::spawn_blocking(move || analyse::restaurer(&fs, vol, &cible, dev)).await.map_err(|e| e.to_string())?;
            let msg = msg.map_err(|e| if e == ERR_ADMIN { l("Écrire sur la carte demande les droits administrateur.") } else { e })?;
            Ok(json!({"ok": true, "message": msg}))
        }
        "/api/fs/stop" => {
            st.fs_stop.store(true, Ordering::Relaxed);
            st.fs.lock().unwrap().log(l("Arrêt demandé..."));
            Ok(json!({"ok": true}))
        }
        "/api/fs/state" => {
            let e = st.fs.lock().unwrap();
            let mut r = json!({
                "running": e.running, "phase": e.phase, "task": e.tache, "started": e.debut.is_some(), "elapsed": ecoule(e.debut, e.fin, e.running),
                "logs": depuis(&e.logs, since), "logCount": e.logs.len(),
                "pDone": e.p_done, "pTotal": e.p_total, "pBad": e.p_bad, "pass": e.pass, "imgPath": e.img_path,
                "vols": e.vols, "fileCount": e.files.len(), "error": e.erreur, "copyDir": e.copie_dir, "ready": e.analyse_ok,
                "version": format!("v{}-{}", e.version, e.files.len()),
            });
            if q.get("files").map(|s| s == "1").unwrap_or(false) {
                r["files"] = json!(e.files);
            } else {
                let copies: HashMap<String, String> = e.files.iter().filter(|f| !f.copied.is_empty()).map(|f| (f.id.to_string(), f.copied.clone())).collect();
                r["copied"] = json!(copies);
            }
            Ok(r)
        }
        _ => Err(format!("route inconnue : {route}")),
    }
}

fn quitter(app: Option<&AppHandle>, ms: u64) {
    let a = app.cloned();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        match a {
            Some(a) => a.exit(0),
            None => std::process::exit(0),
        }
    });
}

// ---------- sélection de fichiers (dialogues natifs) ----------

async fn choisir(app: &AppHandle, st: &App, body: &Value) -> Result<Value, String> {
    let mode = body["mode"].as_str().unwrap_or("folder").to_string();
    let multi = body["multi"].as_bool().unwrap_or(false);
    let titre = body["title"].as_str().unwrap_or("").to_string();
    let mut depart = PathBuf::from(body["start"].as_str().unwrap_or(""));
    if depart.as_os_str().is_empty() || !depart.exists() {
        depart = PathBuf::from(st.cfg.lock().unwrap().last_dir.clone());
    }
    if depart.is_file() {
        depart = depart.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    }
    let mut d = app.dialog().file().set_title(titre);
    if depart.is_dir() {
        d = d.set_directory(&depart);
    }
    let r = tauri::async_runtime::spawn_blocking(move || -> Option<Value> {
        let mut d = d;
        let s = |p: tauri_plugin_dialog::FilePath| p.to_string();
        match mode.as_str() {
            "folder" => d.blocking_pick_folder().map(|p| {
                let p = s(p);
                json!({"folder": p, "dir": p})
            }),
            _ => {
                d = match mode.as_str() {
                    "video" => d.add_filter("Vidéos MOV / MP4", &["mov", "MOV", "mp4", "MP4"]),
                    "image" => d.add_filter("Image disque", &["dd", "img", "raw", "bin", "iso", "001", "dmg"]),
                    "exe" if cfg!(windows) => d.add_filter("Programme", &["exe"]),
                    _ => d,
                };
                let paths: Vec<String> = if multi { d.blocking_pick_files().unwrap_or_default().into_iter().map(s).collect() } else { d.blocking_pick_file().map(s).into_iter().collect() };
                if paths.is_empty() {
                    return None;
                }
                let dir = Path::new(&paths[0]).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                Some(json!({"paths": paths, "dir": dir}))
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Some(dir) = r.as_ref().and_then(|v| v["dir"].as_str()) {
        st.cfg.lock().unwrap().last_dir = dir.to_string();
        st.sauver();
    }
    Ok(r.unwrap_or(Value::Null))
}

// ---------- lancement des traitements ----------

fn demarrer_reparation(st: &App) -> Result<Value, String> {
    if st.rep.lock().unwrap().running {
        return Err(l("Un traitement est déjà en cours."));
    }
    let c = st.cfg.lock().unwrap().clone();
    let fichiers: Vec<PathBuf> = c
        .files
        .iter()
        .map(PathBuf::from)
        .filter(|p| existe_fichier(p))
        .filter(|p| !(c.ignore_ftyp && p.file_stem().map(|s| s.to_string_lossy().to_lowercase().ends_with("_ftyp")).unwrap_or(false)))
        .collect();
    let (untrunc, ffprobe, ffmpeg) = st.outils();
    let sortie = c.sortie.trim().to_string();
    if c.references.is_empty() {
        return Err(l("Ajoutez au moins un fichier de référence sain, filmé avec la même caméra et le même mode."));
    }
    if fichiers.is_empty() {
        return Err(l("Aucun fichier à réparer (liste vide, fichiers introuvables ou tous ignorés)."));
    }
    if sortie.is_empty() {
        return Err(l("Choisissez un dossier de sortie."));
    }
    let untrunc = untrunc.ok_or_else(|| l("untrunc.exe introuvable. Indiquez son emplacement dans Outils."))?;
    let ffprobe = ffprobe.ok_or_else(|| l("ffmpeg / ffprobe introuvables. Indiquez le dossier qui contient ffprobe.exe dans Outils."))?;
    if c.verif && ffmpeg.is_none() {
        return Err(l("ffmpeg / ffprobe introuvables. Indiquez le dossier qui contient ffprobe.exe dans Outils."));
    }
    for r in &c.references {
        if !existe_fichier(Path::new(r)) {
            return Err(format!("{}{r}", l("Référence introuvable : ")));
        }
    }
    let sortie = std::path::absolute(&sortie).unwrap_or_else(|_| PathBuf::from(&sortie));
    if fichiers.iter().any(|f| f.parent() == Some(sortie.as_path())) {
        return Err(l("Le dossier de sortie doit être différent de celui des fichiers à réparer."));
    }
    let job = repair::Job { references: c.references.iter().map(PathBuf::from).collect(), fichiers, sortie: sortie.clone(), untrunc, ffprobe, ffmpeg, verif_complete: c.verif };
    {
        let mut e = st.rep.lock().unwrap();
        *e = repair::Etat { running: true, total: job.fichiers.len(), debut: Some(Instant::now()), ..Default::default() };
        e.log(ataecina_core::i18n::f("{} fichier(s), {} référence(s), sortie : {}", &[&job.fichiers.len(), &job.references.len(), &sortie.display()]));
        if job.verif_complete {
            e.log(l("Vérification complète activée."));
        }
    }
    st.rep_stop.store(false, Ordering::Relaxed);
    let (etat, stop) = (st.rep.clone(), st.rep_stop.clone());
    let total = job.fichiers.len();
    std::thread::spawn(move || {
        repair::executer(&job, &etat, &stop);
        let mut e = etat.lock().unwrap();
        if stop.load(Ordering::Relaxed) {
            e.log(l("Traitement arrêté."));
        }
        e.running = false;
        e.fin = Some(Instant::now());
        e.courant.clear();
    });
    Ok(json!({"ok": true, "total": total}))
}

fn envoyer_trouves(st: &App) -> usize {
    let paths: Vec<String> = {
        let mut e = st.rec.lock().unwrap();
        e.envoye = true;
        e.trouves.iter().filter(|t| photorec::reparable(Path::new(&t.path))).map(|t| t.path.clone()).collect()
    };
    let mut c = st.cfg.lock().unwrap();
    let avant = c.files.len();
    let mut l2 = c.files.clone();
    l2.extend(paths);
    c.files = uniques(l2);
    c.files.len() - avant
}

fn demarrer_photorec(st: &App) -> Result<Value, String> {
    if st.rec.lock().unwrap().running {
        return Err(l("Une récupération est déjà en cours."));
    }
    let rc = st.cfg.lock().unwrap().rec.clone();
    let pr = tools::trouver_photorec(&rc.photorec);
    let (source, drive, img_dir, session, image_seul) = photorec::preparer(&rc, pr.as_deref()).map_err(erreur_admin)?;
    {
        let mut e = st.rec.lock().unwrap();
        *e = photorec::Etat {
            running: true,
            phase: if drive && rc.make_image { "image".into() } else { "photorec".into() },
            debut: Some(Instant::now()),
            session: session.to_string_lossy().to_string(),
            image_seul,
            ..Default::default()
        };
    }
    st.rec_stop.store(false, Ordering::Relaxed);
    let (etat, stop) = (st.rec.clone(), st.rec_stop.clone());
    let (cfg, cfg_path) = (st.cfg.clone(), st.cfg_path.clone());
    std::thread::spawn(move || {
        let res = photorec::executer(&rc, pr.as_deref(), source, drive, &img_dir, &session, &etat, &stop);
        let mut e = etat.lock().unwrap();
        e.running = false;
        e.fin = Some(Instant::now());
        if stop.load(Ordering::Relaxed) {
            e.phase = "stopped".into();
            e.log(l("Récupération arrêtée."));
        } else if let Err(err) = res {
            e.phase = "error".into();
            e.erreur = err.clone();
            e.log(format!("{}{err}", l("ERREUR : ")));
        } else {
            e.phase = "done".into();
            if rc.auto_send {
                let paths: Vec<String> = e.trouves.iter().filter(|t| photorec::reparable(Path::new(&t.path))).map(|t| t.path.clone()).collect();
                let raw = e.trouves.len() - paths.len();
                if !paths.is_empty() {
                    let n = {
                        let mut c = cfg.lock().unwrap();
                        let avant = c.files.len();
                        let mut l2 = c.files.clone();
                        l2.extend(paths);
                        c.files = uniques(l2);
                        c.files.len() - avant
                    };
                    sauver_config(&cfg, &cfg_path);
                    e.envoye = true;
                    if n > 0 {
                        e.log(ataecina_core::i18n::f("{} fichier(s) MOV / MP4 ajouté(s) à la liste de réparation.", &[&n]));
                    }
                }
                if raw > 0 {
                    e.log(ataecina_core::i18n::f("{} fichier(s) RAW (R3D, ARRIRAW, MXF, BRAW) récupéré(s) : ils ne passent pas par untrunc.", &[&raw]));
                }
            }
        }
    });
    Ok(json!({"ok": true}))
}

fn demarrer_analyse(st: &App) -> Result<Value, String> {
    if st.fs.lock().unwrap().running {
        return Err(l("Une analyse ou une copie est déjà en cours."));
    }
    let rc = st.cfg.lock().unwrap().rec.clone();
    let (source, drive, img_dir) = analyse::preparer(&rc).map_err(erreur_admin)?;
    {
        let mut e = st.fs.lock().unwrap();
        let version = e.version + 1;
        *e = analyse::Etat {
            running: true,
            tache: "analyse".into(),
            phase: if drive && rc.make_image { "image".into() } else { "lecture".into() },
            debut: Some(Instant::now()),
            source: source.clone(),
            device: drive,
            version,
            ..Default::default()
        };
    }
    st.fs_stop.store(false, Ordering::Relaxed);
    let (etat, stop) = (st.fs.clone(), st.fs_stop.clone());
    std::thread::spawn(move || {
        let res = analyse::analyser(&rc, source, drive, &img_dir, &etat, &stop);
        let mut e = etat.lock().unwrap();
        e.running = false;
        e.fin = Some(Instant::now());
        if stop.load(Ordering::Relaxed) {
            e.phase = "stopped".into();
            e.log(l("Analyse arrêtée."));
        } else if let Err(err) = res {
            e.phase = "error".into();
            e.erreur = err.clone();
            e.log(format!("{}{err}", l("ERREUR : ")));
        } else {
            e.phase = "done".into();
            e.analyse_ok = true;
        }
    });
    Ok(json!({"ok": true}))
}

fn demarrer_copie(st: &App, body: &Value) -> Result<Value, String> {
    let ids: Vec<usize> = body["ids"].as_array().map(|a| a.iter().filter_map(|x| x.as_u64().map(|v| v as usize)).collect()).unwrap_or_default();
    let rc = st.cfg.lock().unwrap().rec.clone();
    let (liste, total, dir) = analyse::preparer_copie(&st.fs, &ids, &rc)?;
    {
        let mut e = st.fs.lock().unwrap();
        e.running = true;
        e.tache = "copie".into();
        e.phase = "copie".into();
        e.p_done = 0;
        e.p_total = total;
        e.copie_dir = dir.to_string_lossy().to_string();
        e.erreur.clear();
        e.debut = Some(Instant::now());
        e.fin = None;
        let n = liste.len();
        e.log(ataecina_core::i18n::f("Copie de {} fichier(s) ({}) vers {}", &[&n, &ataecina_core::fmt_bytes(total), &dir.display()]));
    }
    st.fs_stop.store(false, Ordering::Relaxed);
    let (_, ffprobe, _) = st.outils();
    let (etat, stop) = (st.fs.clone(), st.fs_stop.clone());
    let auto = rc.auto_send;
    let (cfg, cfg_path) = (st.cfg.clone(), st.cfg_path.clone());
    std::thread::spawn(move || {
        let res = analyse::copier(&liste, &dir, ffprobe.as_deref(), &etat, &stop);
        let mut e = etat.lock().unwrap();
        e.running = false;
        e.fin = Some(Instant::now());
        match res {
            _ if stop.load(Ordering::Relaxed) => {
                e.phase = "stopped".into();
                e.log(l("Copie arrêtée."));
            }
            Err(err) => {
                e.phase = "error".into();
                e.erreur = err.clone();
                e.log(format!("{}{err}", l("ERREUR : ")));
            }
            Ok(illisibles) => {
                e.phase = "done".into();
                if auto && !illisibles.is_empty() {
                    let n = {
                        let mut c = cfg.lock().unwrap();
                        let avant = c.files.len();
                        let mut l2 = c.files.clone();
                        l2.extend(illisibles.iter().map(|p| p.to_string_lossy().to_string()));
                        c.files = uniques(l2);
                        c.files.len() - avant
                    };
                    sauver_config(&cfg, &cfg_path);
                    e.log(ataecina_core::i18n::f("{} vidéo(s) illisible(s) ajoutée(s) à la liste de réparation.", &[&n]));
                }
            }
        }
    });
    Ok(json!({"ok": true}))
}

/// Serveur de test (option de compilation « serveur-test ») : sert l'interface et l'API en HTTP
/// pour piloter l'application depuis un navigateur. Jamais présent dans la version distribuée.
#[cfg(feature = "serveur-test")]
fn serveur_test(addr: &str) {
    let app = App::default();
    let p = std::env::var("ATAECINA_CONFIG").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("config.json"));
    if let Ok(s) = std::fs::read_to_string(&p) {
        if let Ok(c) = serde_json::from_str::<Config>(&s) {
            ataecina_core::i18n::set_lang(&c.lang);
            *app.cfg.lock().unwrap() = c;
        }
    }
    *app.cfg_path.lock().unwrap() = p;
    let ui = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ui");
    let server = tiny_http::Server::http(addr).expect("serveur");
    println!("http://{addr}/");
    for mut req in server.incoming_requests() {
        let url = req.url().to_string();
        if url.starts_with("/api/") {
            let mut b = String::new();
            let _ = std::io::Read::read_to_string(req.as_reader(), &mut b);
            let body = serde_json::from_str(&b).ok();
            let res = tauri::async_runtime::block_on(traiter(&app, None, url, body));
            let (code, v) = match res {
                Ok(v) => (200, v),
                Err(e) => (400, json!({"error": e})),
            };
            let h = tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
            let _ = req.respond(tiny_http::Response::from_string(v.to_string()).with_status_code(code).with_header(h));
        } else {
            let chemin = url.split('?').next().unwrap_or("/");
            let f = ui.join(if chemin == "/" { "index.html" } else { chemin.trim_start_matches('/') });
            let ct = match f.extension().and_then(|e| e.to_str()) {
                Some("html") => "text/html; charset=utf-8",
                Some("js") => "text/javascript; charset=utf-8",
                Some("png") => "image/png",
                _ => "application/octet-stream",
            };
            match std::fs::read(&f) {
                Ok(d) => {
                    let h = tiny_http::Header::from_bytes("Content-Type", ct).unwrap();
                    let _ = req.respond(tiny_http::Response::from_data(d).with_header(h));
                }
                Err(_) => {
                    let _ = req.respond(tiny_http::Response::empty(404));
                }
            }
        }
    }
}

fn main() {
    #[cfg(feature = "serveur-test")]
    if let Ok(addr) = std::env::var("ATAECINA_HTTP") {
        return serveur_test(&addr);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(App::default())
        .setup(|app| {
            let st = app.state::<App>();
            let dir = app.path().app_config_dir().unwrap_or_else(|_| PathBuf::from("."));
            let p = dir.join("config.json");
            if let Ok(s) = std::fs::read_to_string(&p) {
                if let Ok(c) = serde_json::from_str::<Config>(&s) {
                    ataecina_core::i18n::set_lang(&c.lang);
                    *st.cfg.lock().unwrap() = c;
                }
            }
            *st.cfg_path.lock().unwrap() = p;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("Ataecina failed to start");
}
