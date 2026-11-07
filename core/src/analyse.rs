//! Lecture de la carte : analyse du système de fichiers, copie des fichiers choisis,
//! restauration du secteur de démarrage.

use crate::fsread::{self, Fichier, Volume};
use crate::i18n::{f, l};
use crate::imager::faire_image;
use crate::photorec::{RecConfig, ERR_ADMIN};
use crate::platform;
use crate::source::FileSource;
use crate::tools;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Default)]
pub struct Etat {
    pub running: bool,
    pub phase: String, // image, lecture, approfondie, copie, done, error, stopped
    pub tache: String, // analyse, copie
    pub logs: Vec<String>,
    pub debut: Option<Instant>,
    pub fin: Option<Instant>,
    pub p_done: u64,
    pub p_total: u64,
    pub p_bad: u64,
    pub pass: String,
    pub source: PathBuf,
    pub device: bool,
    pub img_path: String,
    pub vols: Vec<Volume>,
    pub files: Vec<Fichier>,
    pub erreur: String,
    pub copie_dir: String,
    pub analyse_ok: bool,
    /// change à chaque nouvelle liste de fichiers (l'interface la recharge alors)
    pub version: u64,
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

pub fn est_video(n: &str) -> bool {
    let e = n.rsplit_once('.').map(|x| x.1.to_lowercase()).unwrap_or_default();
    ["mov", "mp4", "braw", "r3d", "ari", "mxf", "mts"].contains(&e.as_str())
}

/// Vérifie les réglages. Renvoie (source, lecture directe, dossier image).
pub fn preparer(rc: &RecConfig) -> Result<(PathBuf, bool, PathBuf), String> {
    let drive = rc.source_kind != "image";
    if !drive {
        if !crate::existe_fichier(Path::new(&rc.image)) {
            return Err(l("Fichier image introuvable."));
        }
        return Ok((PathBuf::from(&rc.image), false, PathBuf::new()));
    }
    if rc.drive.is_empty() {
        return Err(l("Choisissez la carte ou le disque à analyser."));
    }
    if cfg!(windows) && !platform::est_admin() {
        return Err(ERR_ADMIN.into());
    }
    let source = PathBuf::from(platform::chemin_peripherique(&rc.drive));
    let mut img_dir = PathBuf::new();
    if rc.make_image {
        let d = if !rc.image_dir.trim().is_empty() { rc.image_dir.trim() } else { rc.dest.trim() };
        if d.is_empty() {
            return Err(l("Choisissez où enregistrer l'image (dossier image ou destination)."));
        }
        img_dir = std::path::absolute(d).unwrap_or_else(|_| PathBuf::from(d));
        if cfg!(windows) && platform::volume_de(&img_dir).eq_ignore_ascii_case(&rc.drive) {
            return Err(l("L'image ne peut pas être enregistrée sur la carte elle-même."));
        }
        std::fs::create_dir_all(&img_dir).map_err(|e| e.to_string())?;
        let taille = platform::taille_volume(&rc.drive);
        let libre = platform::espace_libre(&img_dir);
        if taille > 0 && libre > 0 && libre < taille + (64 << 20) {
            return Err(f("Pas assez de place pour l'image : il faut {}, il reste {}.", &[&crate::fmt_bytes(taille), &crate::fmt_bytes(libre)]));
        }
    }
    Ok((source, true, img_dir))
}

pub fn analyser(rc: &RecConfig, source: PathBuf, drive: bool, img_dir: &Path, etat: &Mutex<Etat>, stop: &AtomicBool) -> Result<(), String> {
    let log = |s: String| etat.lock().unwrap().log(s);
    if drive {
        log(f("Source : carte {}", &[&rc.drive]));
    } else {
        log(format!("{}{}", l("Source : image "), source.display()));
    }
    let (mut source, mut device) = (source, drive);
    if drive && rc.make_image {
        let stamp = chrono::Local::now().format("%Y-%m-%d_%Hh%M").to_string();
        let img = faire_image(
            stop,
            &rc.drive,
            &source,
            img_dir,
            &stamp,
            &mut |s| log(s),
            &mut |x| {
                let mut e = etat.lock().unwrap();
                e.p_done = x.fait;
                e.p_total = x.total;
                e.p_bad = x.bad;
                e.pass = x.passe;
            },
            &mut |p| etat.lock().unwrap().img_path = p.to_string_lossy().to_string(),
        )?;
        source = img;
        device = false;
    }
    {
        let mut e = etat.lock().unwrap();
        e.phase = "lecture".into();
        e.source = source.clone();
        e.device = device;
        e.p_done = 0;
        e.p_total = 0;
    }
    let r = FileSource::ouvrir(&source, device).map_err(|e| f("lecture impossible de {} : {}", &[&source.display(), &e]))?;

    log(l("Recherche des volumes exFAT / FAT32..."));
    let mut vols = fsread::trouver_volumes(&r, stop, &mut |s| log(s));
    if stop.load(Ordering::Relaxed) {
        return Err("stop".into());
    }
    if vols.is_empty() {
        return Err(l("Aucun système de fichiers exFAT ou FAT32 n'a été trouvé. La carte est peut-être dans un autre format (NTFS, HFS+, UDF) ou trop abîmée : essayez la méthode PhotoRec."));
    }
    let mut tous: Vec<Fichier> = vec![];
    for v in vols.iter_mut() {
        let kind = if v.kind == fsread::Kind::ExFat { "exFAT" } else { "FAT32" };
        log(f(
            "Volume {} trouvé ({}) : {}, clusters de {}, secteur de démarrage {}.",
            &[&kind, &v.origin, &crate::fmt_bytes(v.size), &crate::fmt_bytes(v.cluster), &v.boot],
        ));
        for n in &v.notes {
            log(format!("{}{n}", l("Attention : ")));
        }
        let fs = v.lire(&r);
        let nf = fs.iter().filter(|x| !x.dir).count();
        let nd = fs.iter().filter(|x| !x.dir && x.deleted).count();
        if !v.label.is_empty() {
            log(f("Nom du volume : {}", &[&v.label]));
        }
        log(f("{} fichier(s) lu(s), dont {} supprimé(s).", &[&nf, &nd]));
        tous.extend(fs);
        if rc.deep {
            etat.lock().unwrap().phase = "approfondie".into();
            log(l("Recherche approfondie des anciennes entrées de fichiers..."));
            let orph = v.recherche_approfondie(&r, &tous, stop, &mut |d, t| {
                let mut e = etat.lock().unwrap();
                e.p_done = d;
                e.p_total = t;
            });
            if stop.load(Ordering::Relaxed) {
                return Err("stop".into());
            }
            log(f("{} fichier(s) supplémentaire(s) retrouvé(s) par la recherche approfondie.", &[&orph.len()]));
            tous.extend(orph);
            etat.lock().unwrap().phase = "lecture".into();
        }
    }
    fsread::trier_fichiers(&mut tous);
    for (i, x) in tous.iter_mut().enumerate() {
        x.id = i;
    }
    let nf = tous.iter().filter(|x| !x.dir).count();
    let nv = tous.iter().filter(|x| !x.dir && est_video(&x.name)).count();
    let mut e = etat.lock().unwrap();
    e.vols = vols;
    e.files = tous;
    e.version += 1;
    e.log(f("Terminé : {} fichier(s) dont {} vidéo(s). Sélectionnez ceux à copier.", &[&nf, &nv]));
    Ok(())
}

// ---------- copie ----------

fn chemin_sur(p: &str) -> PathBuf {
    let mut r = PathBuf::new();
    for c in p.split('/') {
        let c = crate::nom_sur(c);
        let c = c.trim_end_matches(['.', ' ']);
        if c.is_empty() || c == ".." {
            continue;
        }
        r.push(c);
    }
    r
}

fn nom_libre(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let dir = p.parent().unwrap_or(Path::new(".")).to_path_buf();
    (2..).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|q| !q.exists()).unwrap()
}

/// Vérifie la demande de copie. Renvoie (fichiers, total, dossier de copie).
pub fn preparer_copie(etat: &Mutex<Etat>, ids: &[usize], rc: &RecConfig) -> Result<(Vec<Fichier>, u64, PathBuf), String> {
    let e = etat.lock().unwrap();
    if e.running {
        return Err(l("Une opération est déjà en cours."));
    }
    if !e.analyse_ok {
        return Err(l("Lancez d'abord l'analyse."));
    }
    let sel: HashSet<usize> = ids.iter().copied().collect();
    let liste: Vec<Fichier> = e.files.iter().filter(|x| sel.contains(&x.id) && !x.dir).cloned().collect();
    let total: u64 = liste.iter().map(|x| x.size).sum();
    let device = e.device;
    drop(e);
    if liste.is_empty() {
        return Err(l("Aucun fichier sélectionné."));
    }
    let dest = rc.dest.trim();
    if dest.is_empty() {
        return Err(l("Choisissez un dossier de destination."));
    }
    let dest = std::path::absolute(dest).unwrap_or_else(|_| PathBuf::from(dest));
    if device && cfg!(windows) && platform::volume_de(&dest).eq_ignore_ascii_case(&rc.drive) {
        return Err(l("La destination ne doit pas être sur la carte analysée."));
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let libre = platform::espace_libre(&dest);
    if libre > 0 && libre < total {
        return Err(f("Pas assez de place : il faut {}, il reste {}.", &[&crate::fmt_bytes(total), &crate::fmt_bytes(libre)]));
    }
    let dir = dest.join(format!("Fichiers_{}", chrono::Local::now().format("%Y-%m-%d_%Hh%M")));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok((liste, total, dir))
}

/// Copie les fichiers ; renvoie les vidéos illisibles (à envoyer à la réparation).
pub fn copier(liste: &[Fichier], dir: &Path, ffprobe: Option<&Path>, etat: &Mutex<Etat>, stop: &AtomicBool) -> Result<Vec<PathBuf>, String> {
    let (source, device, multi, kinds) = {
        let e = etat.lock().unwrap();
        (e.source.clone(), e.device, e.vols.len() > 1, e.vols.iter().map(|v| v.kind).collect::<Vec<_>>())
    };
    let r = FileSource::ouvrir(&source, device).map_err(|e| f("lecture impossible de {} : {}", &[&source.display(), &e]))?;
    use crate::source::Source;
    let mut fait = 0u64;
    let (mut ok, mut ko) = (0, 0);
    let mut illisibles = vec![];
    const BLOC: u64 = 4 << 20;
    for x in liste {
        if stop.load(Ordering::Relaxed) {
            return Err("stop".into());
        }
        let mut rel = chemin_sur(&x.path);
        if multi {
            let k = if kinds[x.vol] == fsread::Kind::ExFat { "exFAT" } else { "FAT32" };
            rel = PathBuf::from(format!("volume{}_{k}", x.vol + 1)).join(rel);
        }
        let nom = crate::nom_sur(&x.name).trim_end_matches(['.', ' ']).to_string();
        let cible = nom_libre(dir.join(&rel).join(nom));
        if let Some(p) = cible.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&cible).map_err(|e| e.to_string())?;
        let mut ecrit = 0u64;
        for &(pos, n) in &x.runs {
            let mut p = 0u64;
            while p < n {
                if stop.load(Ordering::Relaxed) {
                    return Err("stop".into());
                }
                let k = BLOC.min(n - p);
                let mut b = r.lire(pos + p, k as usize);
                b.resize(k as usize, 0);
                out.write_all(&b).map_err(|e| f("écriture de {} : {}", &[&cible.display(), &e]))?;
                p += k;
                ecrit += k;
                etat.lock().unwrap().p_done = fait + ecrit;
            }
        }
        drop(out);
        fait += x.size;
        if x.mtime > 0 {
            let t = filetime::FileTime::from_unix_time(x.mtime, 0);
            let _ = filetime::set_file_times(&cible, t, t);
        }
        let mut statut = "ok";
        let mut duree = 0.0;
        let ext = cible.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if (ext == "mov" || ext == "mp4") && ffprobe.is_some() {
            duree = tools::duree(ffprobe.unwrap(), &cible, stop);
            if duree <= 0.0 {
                statut = "illisible";
                illisibles.push(cible.clone());
            }
        }
        if ecrit < x.size || x.runs.is_empty() {
            statut = "incomplet";
        }
        {
            let mut e = etat.lock().unwrap();
            if let Some(y) = e.files.iter_mut().find(|y| y.id == x.id) {
                y.copied = statut.into();
                y.dest = cible.to_string_lossy().to_string();
                y.duree = duree;
            }
            if statut == "ok" {
                ok += 1;
                if duree > 0.0 {
                    e.log(f("   {} copié, lisible ({} s)", &[&x.name, &crate::fmt_dec(duree)]));
                } else {
                    e.log(f("   {} copié", &[&x.name]));
                }
            } else {
                ko += 1;
                let quoi = if statut == "illisible" { l("illisible : à réparer") } else { l("incomplet") };
                e.log(f("   {} copié mais {}", &[&x.name, &quoi]));
            }
        }
    }
    etat.lock().unwrap().log(f("Copie terminée : {} fichier(s) bons, {} à vérifier.", &[&ok, &ko]));
    Ok(illisibles)
}

// ---------- restauration du secteur de démarrage ----------

/// Réécrit le secteur de démarrage principal à partir de la copie de secours,
/// sur la carte (`peripherique`) ou dans le fichier image `cible`.
pub fn restaurer(etat: &Mutex<Etat>, vol: usize, cible: &Path, peripherique: bool) -> Result<String, String> {
    let (v, source, device) = {
        let e = etat.lock().unwrap();
        if e.running {
            return Err(l("Une opération est en cours."));
        }
        let v = e.vols.get(vol).cloned().ok_or_else(|| l("Volume inconnu."))?;
        (v, e.source.clone(), e.device)
    };
    if v.main_ok {
        return Err(l("Le secteur de démarrage principal est déjà correct."));
    }
    if peripherique && cfg!(windows) && !platform::est_admin() {
        return Err(ERR_ADMIN.into());
    }
    let r = FileSource::ouvrir(&source, device).map_err(|e| e.to_string())?;
    let (off, data) = fsread::region_restauration(&r, &v).ok_or_else(|| l("Pas de copie de secours utilisable pour ce volume."))?;
    drop(r);
    etat.lock().unwrap().log(f("Restauration du secteur de démarrage de {} à partir de la copie de secours...", &[&cible.display()]));
    if let Err(e) = platform::ecrire_secteurs(cible, off, &data, peripherique) {
        etat.lock().unwrap().log(format!("{}{e}", l("ERREUR : ")));
        return Err(e.to_string());
    }
    let r2 = FileSource::ouvrir(cible, peripherique).map_err(|e| e.to_string())?;
    let ok = fsread::examiner(&r2, off, "verif").map(|w| w.main_ok).unwrap_or(false);
    if !ok {
        etat.lock().unwrap().log(l("ERREUR : la vérification après restauration a échoué."));
        return Err(l("La restauration a été écrite mais la vérification a échoué."));
    }
    let mut msg = l("Secteur de démarrage restauré et vérifié.");
    if peripherique {
        msg += &l(" Retirez puis réinsérez la carte : Windows devrait de nouveau la lire.");
    }
    let mut e = etat.lock().unwrap();
    if let Some(w) = e.vols.get_mut(vol) {
        w.main_ok = true;
        w.boot = l("principal");
        w.notes = vec![l("Secteur de démarrage restauré.")];
    }
    e.log(msg.clone());
    Ok(msg)
}
