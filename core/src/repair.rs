//! Réparation en lot des MOV / MP4 avec untrunc : chaque référence, mode normal puis -s,
//! on garde le meilleur résultat.

use crate::i18n::{f, l};
use crate::tools;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct Job {
    pub references: Vec<PathBuf>,
    pub fichiers: Vec<PathBuf>,
    pub sortie: PathBuf,
    pub untrunc: PathBuf,
    pub ffprobe: PathBuf,
    pub ffmpeg: Option<PathBuf>,
    pub verif_complete: bool,
}

/// Résultat d'un fichier (mêmes noms de champs que l'interface attend).
#[derive(Debug, Clone, Serialize, Default)]
pub struct Resultat {
    #[serde(rename = "Fichier")]
    pub fichier: String,
    #[serde(rename = "Statut")]
    pub statut: String, // OK, INTACT, AVEC_ERREURS, ECHEC, ANNULE
    #[serde(rename = "Reference")]
    pub reference: String,
    #[serde(rename = "Mode")]
    pub mode: String,
    #[serde(rename = "Duree")]
    pub duree: f64,
    #[serde(rename = "Erreurs")]
    pub erreurs: usize,
    #[serde(rename = "Sortie")]
    pub sortie: String,
}

/// État partagé avec l'interface.
#[derive(Debug, Default)]
pub struct Etat {
    pub running: bool,
    pub logs: Vec<String>,
    pub results: Vec<Resultat>,
    pub fait: usize,
    pub total: usize,
    pub courant: String,
    pub debut: Option<Instant>,
    pub fin: Option<Instant>,
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

fn base(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

fn nom_base(p: &Path) -> String {
    let b = base(p);
    let lb = b.to_lowercase();
    for suf in ["_mdat", "_ftyp"] {
        if lb.ends_with(suf) {
            return b[..b.len() - suf.len()].to_string();
        }
    }
    b
}

/// Fichiers produits par untrunc à côté du fichier source.
fn sorties_untrunc(src: &Path) -> Vec<PathBuf> {
    let dir = src.parent().unwrap_or(Path::new("."));
    let b = base(src).to_lowercase();
    let nom = src.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false) && e.file_name() != nom)
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy().to_lowercase();
            n.starts_with(&b) && n[b.len()..].contains("fixed")
        })
        .collect()
}

fn nettoyer(src: &Path) {
    for p in sorties_untrunc(src) {
        let _ = std::fs::remove_file(p);
    }
}

fn deplacer(a: &Path, b: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(b);
    if std::fs::rename(a, b).is_ok() {
        return Ok(());
    }
    std::fs::copy(a, b)?;
    std::fs::remove_file(a)
}

pub fn executer(job: &Job, etat: &Mutex<Etat>, stop: &AtomicBool) {
    let log = |s: String| etat.lock().unwrap().log(s);
    let logs = job.sortie.join("logs");
    if let Err(e) = std::fs::create_dir_all(&logs) {
        log(format!("{}{e}", l("Impossible de créer le dossier de sortie : ")));
        return;
    }
    let total = job.fichiers.len();
    let debut_lot = Instant::now();
    let mut resultats: Vec<Resultat> = vec![];
    for (i, fic) in job.fichiers.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        {
            let mut e = etat.lock().unwrap();
            e.fait = i;
            e.total = total;
            e.courant = fic.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let c = e.courant.clone();
            e.log(format!("[{}/{}] {}", i + 1, total, c));
        }
        let debut = Instant::now();
        let mut r = traiter(job, fic, &logs, etat, stop);
        if stop.load(Ordering::Relaxed) && r.statut != "OK" && r.statut != "INTACT" {
            r.statut = "ANNULE".into();
        }
        let msg = match r.statut.as_str() {
            "INTACT" => f("   déjà lisible ({} s), copié sans réparation", &[&crate::fmt_dec(r.duree)]),
            "OK" | "AVEC_ERREURS" => {
                let mut m = f("   {} : {} s récupérées (référence {}, mode {})", &[&r.statut, &crate::fmt_dec(r.duree), &r.reference, &r.mode]);
                if r.erreurs > 0 {
                    m += &f(", {} erreur(s) de décodage", &[&r.erreurs]);
                }
                m
            }
            "ANNULE" => l("   annulé"),
            _ => l("   ÉCHEC : aucune référence n'a permis de réparer ce fichier"),
        };
        {
            let mut e = etat.lock().unwrap();
            e.log(msg);
            e.log(format!("   ({})", crate::fmt_duree(debut.elapsed())));
            e.results.push(r.clone());
        }
        resultats.push(r);
    }
    ecrire_journal(&job.sortie.join("journal.csv"), &resultats);
    let compte = |s: &str| resultats.iter().filter(|r| r.statut == s).count();
    let annules = compte("ANNULE");
    let mut e = etat.lock().unwrap();
    e.fait = resultats.len();
    e.courant.clear();
    e.log(String::new());
    let suffixe = if annules > 0 { f(", {} annulés", &[&annules]) } else { String::new() };
    e.log(f(
        "Terminé en {} : {} OK, {} intacts, {} avec erreurs, {} échecs{}",
        &[&crate::fmt_duree(debut_lot.elapsed()), &compte("OK"), &compte("INTACT"), &compte("AVEC_ERREURS"), &compte("ECHEC"), &suffixe],
    ));
    e.log(format!("{}{}", l("Journal : "), job.sortie.join("journal.csv").display()));
}

fn traiter(job: &Job, fic: &Path, logs: &Path, etat: &Mutex<Etat>, stop: &AtomicBool) -> Resultat {
    let log = |s: String| etat.lock().unwrap().log(s);
    let ext = fic.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let b = base(fic);
    let nb = nom_base(fic);
    let mut r = Resultat { fichier: fic.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), statut: "ECHEC".into(), ..Default::default() };

    // 1. fichier déjà lisible ?
    let d = tools::duree(&job.ffprobe, fic, stop);
    if d > 0.0 {
        let mut err = 0;
        if job.verif_complete {
            log(l("   lisible, vérification complète..."));
            err = job.ffmpeg.as_ref().map(|m| tools::erreurs(m, fic, stop)).unwrap_or(0);
        }
        if err == 0 && !stop.load(Ordering::Relaxed) {
            let dest = job.sortie.join(format!("{nb}{ext}"));
            if let Err(e) = std::fs::copy(fic, &dest) {
                log(format!("{}{e}", l("   copie impossible : ")));
                return r;
            }
            r.statut = "INTACT".into();
            r.duree = d;
            r.sortie = dest.to_string_lossy().to_string();
            return r;
        }
    }

    // 2. untrunc avec chaque référence, mode normal puis -s
    let dest = job.sortie.join(format!("{nb}_repare.mov"));
    let candidat = job.sortie.join(format!("_candidat_{nb}_repare.mov"));
    let mut meilleur: Option<Resultat> = None;
    'essais: for rf in &job.references {
        for mode in ["normal", "step"] {
            if stop.load(Ordering::Relaxed) {
                break 'essais;
            }
            nettoyer(fic);
            let ref_nom = rf.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            log(f("   untrunc {} avec {}...", &[&mode, &ref_nom]));
            let mut c = Command::new(&job.untrunc);
            if mode == "step" {
                c.arg("-s");
            }
            c.arg(rf).arg(fic);
            if let Some(d) = job.untrunc.parent() {
                c.current_dir(d);
            }
            if let Some((_, out, err)) = tools::executer(&mut c, stop, None) {
                let log_path = logs.join(crate::nom_sur(&format!("{b}__{}__{mode}.log", base(rf))));
                let mut tout = out;
                tout.extend(err);
                let _ = std::fs::write(log_path, tout);
            }
            let Some(out) = sorties_untrunc(fic).into_iter().next() else { continue };
            let d = tools::duree(&job.ffprobe, &out, stop);
            if d <= 0.0 {
                nettoyer(fic);
                continue;
            }
            let mut e = 0;
            if job.verif_complete {
                log(f("   résultat {} s, vérification complète...", &[&crate::fmt_dec(d)]));
                e = job.ffmpeg.as_ref().map(|m| tools::erreurs(m, &out, stop)).unwrap_or(0);
            }
            let mieux = match &meilleur {
                None => true,
                Some(m) => e < m.erreurs || (e == m.erreurs && d > m.duree),
            };
            if mieux {
                if let Err(err) = deplacer(&out, &candidat) {
                    log(format!("{}{err}", l("   déplacement impossible : ")));
                    nettoyer(fic);
                    continue;
                }
                meilleur = Some(Resultat { reference: ref_nom, mode: mode.into(), duree: d, erreurs: e, ..Default::default() });
            }
            nettoyer(fic);
            if e == 0 {
                break 'essais;
            }
        }
    }
    nettoyer(fic);
    let Some(m) = meilleur else { return r };
    if let Err(err) = deplacer(&candidat, &dest) {
        log(format!("{}{err}", l("   déplacement impossible : ")));
        return r;
    }
    r.statut = if m.erreurs == 0 { "OK" } else { "AVEC_ERREURS" }.into();
    r.reference = m.reference;
    r.mode = m.mode;
    r.duree = m.duree;
    r.erreurs = m.erreurs;
    r.sortie = dest.to_string_lossy().to_string();
    r
}

fn ecrire_journal(path: &Path, res: &[Resultat]) {
    let mut s = String::from("\u{FEFF}fichier;statut;reference;mode;duree_s;erreurs_decodage;sortie\r\n");
    for r in res {
        let duree = if r.duree > 0.0 { format!("{:.2}", r.duree).replace('.', ",") } else { String::new() };
        s += &format!("{};{};{};{};{};{};{}\r\n", r.fichier, r.statut, r.reference, r.mode, duree, r.erreurs, r.sortie);
    }
    let _ = std::fs::write(path, s);
}
