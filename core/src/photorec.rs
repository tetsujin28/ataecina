//! Récupération par PhotoRec : image éventuelle de la carte, puis analyse limitée aux formats vidéo choisis.

use crate::i18n::{f, l};
use crate::imager::{faire_image, EtatImage};
use crate::platform;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Réglages de l'onglet Récupération (partagés avec la lecture de carte).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RecConfig {
    #[serde(rename = "sourceKind")]
    pub source_kind: String, // drive | image
    pub drive: String,
    pub image: String,
    #[serde(rename = "makeImage")]
    pub make_image: bool,
    #[serde(rename = "imageDir")]
    pub image_dir: String,
    pub dest: String,
    pub photorec: String,
    #[serde(rename = "autoSend")]
    pub auto_send: bool,
    #[serde(rename = "freeOnly")]
    pub free_only: bool,
    pub formats: Vec<String>, // mov, r3d, ari, mxf
    pub method: String,       // fs, photorec
    pub deep: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trouve {
    pub name: String,
    pub path: String,
    pub dir: String,
    pub size: u64,
    pub kind: String,
}

#[derive(Debug, Default)]
pub struct Etat {
    pub running: bool,
    pub phase: String, // image, photorec, done, error, stopped
    pub logs: Vec<String>,
    pub debut: Option<Instant>,
    pub fin: Option<Instant>,
    pub img: EtatImage,
    pub img_path: String,
    pub session: String,
    pub trouves: Vec<Trouve>,
    pub total_size: u64,
    pub all_files: usize,
    pub erreur: String,
    pub envoye: bool,
    pub image_seul: bool,
}

impl Etat {
    pub fn log(&mut self, s: impl Into<String>) {
        self.logs.push(s.into());
    }
    pub fn elapsed(&self) -> f64 {
        match (self.debut, self.fin) {
            (Some(d), Some(f)) if !self.running => (f - d).as_secs_f64(),
            (Some(d), _) => d.elapsed().as_secs_f64(),
            _ => 0.0,
        }
    }
}

const EXTS_VIDEO: [&str; 6] = ["mov", "mp4", "braw", "r3d", "ari", "mxf"];

pub fn reparable(p: &Path) -> bool {
    matches!(p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref(), Some("mov") | Some("mp4"))
}

fn formats(f: &str) -> Option<(&'static [&'static str], &'static str)> {
    match f {
        "mov" => Some((&["mov"], "MOV / MP4 / BRAW")),
        "r3d" => Some((&["r3d"], "R3D")),
        "ari" => Some((&["ari", "mxf"], "ARRIRAW")),
        "mxf" => Some((&["mxf"], "MXF")),
        _ => None,
    }
}

pub fn scan_trouves(dir: &Path) -> (Vec<Trouve>, u64, usize) {
    let mut res = vec![];
    let (mut total, mut tous) = (0u64, 0usize);
    let mut pile = vec![dir.to_path_buf()];
    while let Some(d) = pile.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                pile.push(p);
                continue;
            }
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext == "log" || ext == "txt" {
                continue;
            }
            tous += 1;
            if !EXTS_VIDEO.contains(&ext.as_str()) {
                continue;
            }
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            let base = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            let kind = if base.ends_with("_mdat") {
                "mdat".into()
            } else if base.ends_with("_ftyp") {
                "ftyp".into()
            } else {
                ext.clone()
            };
            res.push(Trouve {
                name: p.file_name().unwrap().to_string_lossy().to_string(),
                dir: p.parent().and_then(|x| x.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                path: p.to_string_lossy().to_string(),
                size,
                kind,
            });
            total += size;
        }
    }
    res.sort_by(|a, b| a.name.len().cmp(&b.name.len()).then_with(|| a.name.cmp(&b.name)));
    (res, total, tous)
}

/// Erreur spéciale : droits administrateur nécessaires.
pub const ERR_ADMIN: &str = "__admin__";

/// Vérifie les réglages et prépare la session. Renvoie (source, lecture directe, dossier image, dossier de session, image seule).
pub fn preparer(rc: &RecConfig, photorec: Option<&Path>) -> Result<(PathBuf, bool, PathBuf, PathBuf, bool), String> {
    let dest = rc.dest.trim();
    if dest.is_empty() {
        return Err(l("Choisissez un dossier de destination pour les fichiers récupérés."));
    }
    let dest = std::path::absolute(dest).unwrap_or_else(|_| PathBuf::from(dest));
    let drive = rc.source_kind != "image";
    let source;
    if drive {
        if rc.drive.is_empty() {
            return Err(l("Choisissez la carte ou le disque à analyser."));
        }
        if cfg!(windows) && !platform::est_admin() {
            return Err(ERR_ADMIN.into());
        }
        if platform::volume_de(&dest).eq_ignore_ascii_case(&rc.drive) && !rc.drive.is_empty() && cfg!(windows) {
            return Err(l("La destination ne doit pas être sur la carte analysée : on risquerait d'écraser ce qu'on cherche à récupérer."));
        }
        source = PathBuf::from(platform::chemin_peripherique(&rc.drive));
    } else {
        if !crate::existe_fichier(Path::new(&rc.image)) {
            return Err(l("Fichier image introuvable."));
        }
        source = PathBuf::from(&rc.image);
    }
    let image_seul = photorec.is_none();
    if image_seul && !(drive && rc.make_image) {
        return Err(l("PhotoRec introuvable. Indiquez l'emplacement de photorec_win.exe (fourni avec TestDisk), ou activez la création d'image seule."));
    }
    let mut img_dir = PathBuf::new();
    if drive && rc.make_image {
        let d = if rc.image_dir.trim().is_empty() { dest.clone() } else { PathBuf::from(rc.image_dir.trim()) };
        img_dir = std::path::absolute(&d).unwrap_or(d);
        if cfg!(windows) && platform::volume_de(&img_dir).eq_ignore_ascii_case(&rc.drive) {
            return Err(l("L'image ne peut pas être enregistrée sur la carte elle-même."));
        }
        std::fs::create_dir_all(&img_dir).map_err(|e| e.to_string())?;
        let taille = platform::taille_volume(&rc.drive);
        let libre = platform::espace_libre(&img_dir);
        if taille > 0 && libre > 0 && libre < taille + (64 << 20) {
            return Err(f("Pas assez de place pour l'image : il faut {}, il reste {} sur la destination.", &[&crate::fmt_bytes(taille), &crate::fmt_bytes(libre)]));
        }
    }
    let session = dest.join(format!("Recuperation_{}", chrono::Local::now().format("%Y-%m-%d_%Hh%M")));
    std::fs::create_dir_all(&session).map_err(|e| e.to_string())?;
    Ok((source, drive, img_dir, session, image_seul))
}

/// Déroule la récupération (à lancer dans un fil séparé après `preparer`).
pub fn executer(rc: &RecConfig, photorec: Option<&Path>, source: PathBuf, drive: bool, img_dir: &Path, session: &Path, etat: &Mutex<Etat>, stop: &AtomicBool) -> Result<(), String> {
    let log = |s: String| etat.lock().unwrap().log(s);
    if drive {
        log(f("Source : {} ({})", &[&rc.drive, &source.display()]));
    } else {
        log(format!("{}{}", l("Source : image "), source.display()));
    }
    let mut source = source;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%Hh%M").to_string();

    // 1. image
    if drive && rc.make_image {
        let img = faire_image(
            stop,
            &rc.drive,
            &source,
            img_dir,
            &stamp,
            &mut |s| log(s),
            &mut |e| etat.lock().unwrap().img = e,
            &mut |p| etat.lock().unwrap().img_path = p.to_string_lossy().to_string(),
        )?;
        source = img;
        if photorec.is_none() {
            log(l("PhotoRec n'est pas installé : l'image est prête, l'analyse n'a pas été lancée."));
            return Ok(());
        }
    }
    let Some(photorec) = photorec else { return Ok(()) };

    // 2. PhotoRec
    etat.lock().unwrap().phase = "photorec".into();
    let familles = if rc.formats.is_empty() { vec!["mov".to_string()] } else { rc.formats.clone() };
    let mut vus = HashSet::new();
    let (mut modules, mut labels) = (vec![], vec![]);
    for fam in &familles {
        if let Some((mods, label)) = formats(fam) {
            labels.push(label);
            for m in mods {
                if vus.insert(*m) {
                    modules.push(format!("{m},enable"));
                }
            }
        }
    }
    if modules.is_empty() {
        modules.push("mov,enable".into());
        labels.push("MOV / MP4");
    }
    let espace = if rc.free_only { "freespace" } else { "wholespace" };
    let cmd_pr = format!("partition_none,options,keep_corrupted_file,fileopt,everything,disable,{},{espace},search", modules.join(","));
    log(format!("{}{}...", l("Analyse de la carte avec PhotoRec : "), labels.join(", ")));

    let args: Vec<std::ffi::OsString> = vec![
        "/log".into(),
        "/logname".into(),
        session.join("photorec.log").into(),
        "/d".into(),
        session.join("recup_dir").into(),
        "/cmd".into(),
        source.clone().into(),
        cmd_pr.into(),
    ];
    let mut ch = platform::lancer_invisible(photorec, &args, session).map_err(|e| f("impossible de lancer PhotoRec : {}", &[&e]))?;

    let mut deja: HashSet<String> = HashSet::new();
    let mut maj = || {
        let (t, total, tous) = scan_trouves(session);
        let mut e = etat.lock().unwrap();
        for x in &t {
            if deja.insert(x.path.clone()) {
                e.log(f("   trouvé {} ({})", &[&x.name, &crate::fmt_bytes(x.size)]));
            }
        }
        e.trouves = t;
        e.total_size = total;
        e.all_files = tous;
    };
    let statut = loop {
        if stop.load(Ordering::Relaxed) {
            ch.tuer();
            maj();
            return Err("stop".into());
        }
        match ch.fini() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        maj();
        std::thread::sleep(Duration::from_secs(1));
    };
    maj();
    let (n, total) = {
        let e = etat.lock().unwrap();
        (e.trouves.len(), e.total_size)
    };
    if statut != Some(0) && n == 0 {
        return Err(f("PhotoRec s'est arrêté avec une erreur ({}). Consultez photorec.log dans {}", &[&statut.map(|s| format!("code {s}")).unwrap_or_default(), &session.display()]));
    }
    log(f("Terminé : {} fichier(s) vidéo récupéré(s), {}.", &[&n, &crate::fmt_bytes(total)]));
    Ok(())
}
