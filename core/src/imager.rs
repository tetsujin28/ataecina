#![allow(unused_assignments, dead_code)]
//! Création d'image de carte selon la méthode de GNU ddrescue :
//!   1. copie rapide par gros blocs, en sautant les zones qui résistent
//!   2. copie inverse des zones sautées
//!   3. rognage : on réduit chaque zone en échec par ses deux bords, secteur par secteur
//!   4. raclage : on lit un par un les secteurs restants
//!   5. réessais sur les secteurs illisibles
//! Un fichier de suivi (.map, au format ddrescue) permet de reprendre une copie interrompue.

use crate::i18n::{f, l};
use crate::source::{read_full, write_at};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const NON_LU: u8 = b'?';
const A_ROGNER: u8 = b'*';
const A_RACLER: u8 = b'/';
const ILLISIBLE: u8 = b'-';
const OK: u8 = b'+';
const SECTEUR: u64 = 512;
const GROS_BLOC: u64 = 1 << 20;
const NB_REESSAIS: u32 = 2;

#[derive(Debug, Clone, Serialize, Default)]
pub struct EtatImage {
    pub fait: u64,
    pub total: u64,
    pub bad: u64,
    pub reste: u64,
    pub passe: String,
    pub reprise: bool,
}

#[derive(Debug, Clone, Copy)]
struct Zone {
    pos: u64,
    size: u64,
    st: u8,
}

pub struct CarteZones {
    z: Vec<Zone>,
    total: u64,
}

impl CarteZones {
    pub fn nouvelle(total: u64) -> Self {
        CarteZones { z: vec![Zone { pos: 0, size: total, st: NON_LU }], total }
    }

    /// Donne le statut `st` à l'intervalle [pos, pos+size).
    fn marquer(&mut self, pos: u64, size: u64, st: u8) {
        if size == 0 {
            return;
        }
        let fin = pos + size;
        let mut res = Vec::with_capacity(self.z.len() + 2);
        for z in &self.z {
            let zf = z.pos + z.size;
            if zf <= pos || z.pos >= fin {
                res.push(*z);
                continue;
            }
            if z.pos < pos {
                res.push(Zone { pos: z.pos, size: pos - z.pos, st: z.st });
            }
            if zf > fin {
                res.push(Zone { pos: fin, size: zf - fin, st: z.st });
            }
        }
        res.push(Zone { pos, size, st });
        res.sort_by_key(|z| z.pos);
        let mut m: Vec<Zone> = Vec::with_capacity(res.len());
        for z in res {
            match m.last_mut() {
                Some(d) if d.st == z.st && d.pos + d.size == z.pos => d.size += z.size,
                _ => m.push(z),
            }
        }
        self.z = m;
    }

    pub fn somme(&self, sts: &[u8]) -> u64 {
        self.z.iter().filter(|z| sts.contains(&z.st)).map(|z| z.size).sum()
    }

    fn zones(&self, st: u8) -> Vec<Zone> {
        self.z.iter().filter(|z| z.st == st).copied().collect()
    }

    pub fn sauver(&self, path: &Path, passe: &str) -> io::Result<()> {
        let mut s = String::from("# Mapfile créé par Ataecina (format GNU ddrescue)\n");
        s += &format!("# Passe : {passe}\n");
        s += "# current_pos  current_status  current_pass\n0x00000000     ?               1\n";
        s += "#      pos        size  status\n";
        for z in &self.z {
            s += &format!("0x{:08X}  0x{:08X}  {}\n", z.pos, z.size, z.st as char);
        }
        let tmp = path.with_extension("map.tmp");
        std::fs::write(&tmp, s)?;
        std::fs::rename(tmp, path)
    }

    pub fn charger(path: &Path, total: u64) -> Option<Self> {
        let f = File::open(path).ok()?;
        let mut z = vec![];
        let mut entete = false;
        for ligne in BufReader::new(f).lines() {
            let ligne = ligne.ok()?;
            let t = ligne.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            if !entete {
                entete = true; // ligne current_pos
                continue;
            }
            let ch: Vec<&str> = t.split_whitespace().collect();
            if ch.len() != 3 || ch[2].len() != 1 {
                return None;
            }
            let pos = u64::from_str_radix(ch[0].trim_start_matches("0x"), 16).ok()?;
            let size = u64::from_str_radix(ch[1].trim_start_matches("0x"), 16).ok()?;
            z.push(Zone { pos, size, st: ch[2].as_bytes()[0] });
        }
        let mut fin = 0;
        for x in &z {
            if x.pos != fin {
                return None;
            }
            fin += x.size;
        }
        if fin != total || z.is_empty() {
            return None;
        }
        Some(CarteZones { z, total })
    }
}

// ---------- source ----------

pub trait Lecteur: Send {
    /// Lit exactement buf.len() octets à `off` ; false en cas d'erreur de lecture.
    fn lire_exact(&self, buf: &mut [u8], off: u64) -> bool;
    fn taille(&self) -> u64;
}

struct LecteurFichier {
    f: File,
    taille: u64,
}

impl Lecteur for LecteurFichier {
    fn lire_exact(&self, buf: &mut [u8], off: u64) -> bool {
        let (n, e) = read_full(&self.f, buf, off);
        n == buf.len() && e.is_none()
    }
    fn taille(&self) -> u64 {
        self.taille
    }
}

/// Carte simulée défectueuse, pour les tests : ATAECINA_BADRANGES="debut:longueur:essais,..."
/// Les lectures qui touchent une zone échouent ; « essais » = nombre de lectures ratées par
/// secteur avant succès (0 = toujours illisible).
struct LecteurDefectueux {
    inner: LecteurFichier,
    zones: Vec<(u64, u64, u32)>,
    echecs: Mutex<HashMap<u64, u32>>,
}

impl Lecteur for LecteurDefectueux {
    fn lire_exact(&self, buf: &mut [u8], off: u64) -> bool {
        let fin = off + buf.len() as u64;
        for &(a, n, essais) in &self.zones {
            if off < a + n && fin > a {
                std::thread::sleep(Duration::from_millis(1));
                if buf.len() as u64 > SECTEUR || essais == 0 {
                    return false;
                }
                let mut e = self.echecs.lock().unwrap();
                let c = e.entry(off).or_insert(0);
                *c += 1;
                if *c <= essais {
                    return false;
                }
            }
        }
        self.inner.lire_exact(buf, off)
    }
    fn taille(&self) -> u64 {
        self.inner.taille
    }
}

pub fn ouvrir_lecteur(src: &Path) -> io::Result<Box<dyn Lecteur>> {
    let f = File::open(src)?;
    let taille = crate::platform::taille_source(&f)?;
    if taille == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "taille"));
    }
    let base = LecteurFichier { f, taille };
    if let Ok(spec) = std::env::var("ATAECINA_BADRANGES") {
        let zones = spec
            .split(',')
            .filter_map(|z| {
                let p: Vec<u64> = z.split(':').filter_map(|x| x.parse().ok()).collect();
                (p.len() == 3).then(|| (p[0], p[1], p[2] as u32))
            })
            .collect();
        return Ok(Box::new(LecteurDefectueux { inner: base, zones, echecs: Mutex::new(HashMap::new()) }));
    }
    Ok(Box::new(base))
}

/// Cherche dans `dir` une image interrompue de la même carte.
pub fn image_a_reprendre(dir: &Path, prefixe: &str, total: u64) -> Option<PathBuf> {
    let mut maps: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            n.starts_with(prefixe) && n.ends_with(".dd.map")
        })
        .collect();
    maps.sort();
    maps.reverse();
    for m in maps {
        let img = m.with_extension(""); // retire .map
        if std::fs::metadata(&img).map(|x| x.len()).ok() != Some(total) {
            continue;
        }
        if let Some(c) = CarteZones::charger(&m, total) {
            if c.somme(b"?*/") > 0 {
                return Some(img);
            }
        }
    }
    None
}

/// Crée (ou reprend) l'image de `src` dans `dst`.
pub fn copier_image(stop: &AtomicBool, src: &Path, dst: &Path, maj: &mut dyn FnMut(EtatImage)) -> Result<(), String> {
    let lect = ouvrir_lecteur(src).map_err(|e| f("lecture impossible de {} : {}", &[&src.display(), &e]))?;
    let total = lect.taille();
    let map_path = PathBuf::from(format!("{}.map", dst.display()));
    let mut reprise = false;
    let mut carte = None;
    if std::fs::metadata(dst).map(|m| m.len()).ok() == Some(total) {
        carte = CarteZones::charger(&map_path, total);
        reprise = carte.is_some();
    }
    let mut carte = carte.unwrap_or_else(|| CarteZones::nouvelle(total));
    let out = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dst).map_err(|e| e.to_string())?;
    if !reprise {
        out.set_len(total).map_err(|e| f("création de l'image : {}", &[&e]))?;
    }

    let mut passe = l("Copie rapide");
    let mut dernier = Instant::now() - Duration::from_secs(1);
    let mut dernier_map = Instant::now();
    let mut buf = vec![0u8; GROS_BLOC as usize];

    macro_rules! etat {
        ($force:expr) => {
            if $force || dernier.elapsed() >= Duration::from_millis(250) {
                dernier = Instant::now();
                maj(EtatImage {
                    fait: carte.somme(b"+"),
                    total,
                    bad: carte.somme(b"-"),
                    reste: carte.somme(b"?*/"),
                    passe: passe.clone(),
                    reprise,
                });
                if $force || dernier_map.elapsed() >= Duration::from_secs(3) {
                    let _ = carte.sauver(&map_path, &passe);
                    dernier_map = Instant::now();
                }
            }
        };
    }
    macro_rules! arret {
        () => {
            if stop.load(Ordering::Relaxed) {
                etat!(true);
                return Err("stop".into());
            }
        };
    }
    let lire_ecrire = |pos: u64, n: u64, buf: &mut Vec<u8>| -> bool {
        let b = &mut buf[..n as usize];
        lect.lire_exact(b, pos) && write_at(&out, b, pos).is_ok()
    };

    // 1. copie rapide, avec sauts croissants après une erreur
    for z in carte.zones(NON_LU) {
        let mut saut = 64u64 << 10;
        let fin = z.pos + z.size;
        let mut pos = z.pos;
        while pos < fin {
            arret!();
            let n = (GROS_BLOC - pos % GROS_BLOC).min(fin - pos);
            if lire_ecrire(pos, n, &mut buf) {
                carte.marquer(pos, n, OK);
                pos += n;
                saut = 64 << 10;
            } else {
                carte.marquer(pos, n, A_ROGNER);
                // on laisse une zone non lue derrière, reprise par la passe inverse
                pos = (pos + n + saut).min(fin);
                saut = (saut * 2).min(64 << 20);
            }
            etat!(false);
        }
    }

    // 2. copie inverse des zones sautées, sans saut
    passe = l("Copie inverse");
    for z in carte.zones(NON_LU) {
        let mut fin = z.pos + z.size;
        while fin > z.pos {
            arret!();
            let mut n = fin - z.pos;
            let r = fin % GROS_BLOC;
            if r != 0 && r < n {
                n = r;
            }
            let n = n.min(GROS_BLOC);
            let pos = fin - n;
            if lire_ecrire(pos, n, &mut buf) {
                carte.marquer(pos, n, OK);
            } else {
                carte.marquer(pos, n, A_ROGNER);
            }
            fin = pos;
            etat!(false);
        }
    }

    // 3. rognage : par les bords, jusqu'au premier secteur illisible de chaque côté
    passe = l("Rognage");
    for z in carte.zones(A_ROGNER) {
        let (mut debut, mut fin) = (z.pos, z.pos + z.size);
        while debut < fin {
            arret!();
            let n = SECTEUR.min(fin - debut);
            if !lire_ecrire(debut, n, &mut buf) {
                carte.marquer(debut, n, ILLISIBLE);
                debut += n;
                break;
            }
            carte.marquer(debut, n, OK);
            debut += n;
            etat!(false);
        }
        while fin > debut {
            arret!();
            let n = SECTEUR.min(fin - debut);
            if !lire_ecrire(fin - n, n, &mut buf) {
                carte.marquer(fin - n, n, ILLISIBLE);
                fin -= n;
                break;
            }
            carte.marquer(fin - n, n, OK);
            fin -= n;
            etat!(false);
        }
        if fin > debut {
            carte.marquer(debut, fin - debut, A_RACLER);
        }
    }

    // 4. raclage : chaque secteur restant
    passe = l("Raclage");
    for z in carte.zones(A_RACLER) {
        let mut pos = z.pos;
        while pos < z.pos + z.size {
            arret!();
            let n = SECTEUR.min(z.pos + z.size - pos);
            let st = if lire_ecrire(pos, n, &mut buf) { OK } else { ILLISIBLE };
            carte.marquer(pos, n, st);
            pos += n;
            etat!(false);
        }
    }

    // 5. réessais, en alternant le sens de lecture
    for essai in 1..=NB_REESSAIS {
        let bad = carte.zones(ILLISIBLE);
        if bad.is_empty() {
            break;
        }
        passe = f("Réessai {}/{}", &[&essai, &NB_REESSAIS]);
        let mut secteurs: Vec<u64> = bad.iter().flat_map(|z| (z.pos..z.pos + z.size).step_by(SECTEUR as usize)).collect();
        if essai % 2 == 1 {
            secteurs.reverse();
        }
        for p in secteurs {
            arret!();
            let n = SECTEUR.min(total - p);
            if lire_ecrire(p, n, &mut buf) {
                carte.marquer(p, n, OK);
            }
            etat!(false);
        }
    }
    passe = l("Terminé");
    out.sync_all().map_err(|e| e.to_string())?;
    etat!(true);
    Ok(())
}

/// Crée ou reprend l'image d'une carte et journalise le déroulement.
pub fn faire_image(
    stop: &AtomicBool,
    drive: &str,
    source: &Path,
    img_dir: &Path,
    stamp: &str,
    log: &mut dyn FnMut(String),
    maj: &mut dyn FnMut(EtatImage),
    chemin: &mut dyn FnMut(PathBuf),
) -> Result<PathBuf, String> {
    let mut lettre = drive.trim_end_matches([':', '\\', '/']).to_string();
    if lettre.len() > 2 {
        lettre = Path::new(&lettre).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(lettre);
    }
    let prefixe = format!("carte_{}_", crate::nom_sur(&lettre));
    let mut img = img_dir.join(format!("{prefixe}{stamp}.dd"));
    if let Ok(lect) = ouvrir_lecteur(source) {
        if let Some(p) = image_a_reprendre(img_dir, &prefixe, lect.taille()) {
            img = p;
            log(format!("{}{}", l("Reprise de l'image interrompue : "), img.display()));
        }
    }
    chemin(img.clone());
    log(format!("{}{}", l("Image de la carte : "), img.display()));
    let debut = Instant::now();
    let mut der = EtatImage::default();
    let res = copier_image(stop, source, &img, &mut |e| {
        der = e.clone();
        maj(e)
    });
    if let Err(e) = res {
        if stop.load(Ordering::Relaxed) {
            log(l("Image interrompue : relancez avec la même carte et le même dossier pour la reprendre là où elle s'est arrêtée."));
        }
        return Err(e);
    }
    log(f("Image terminée : {} en {}", &[&crate::fmt_bytes(der.total), &crate::fmt_duree(debut.elapsed())]));
    if der.bad > 0 {
        let nom = img.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        log(f(
            "Attention : {} illisibles sur la carte malgré les réessais, remplacés par des zéros (détail dans {}.map).",
            &[&crate::fmt_bytes(der.bad), &nom],
        ));
    } else {
        log(l("Aucun secteur illisible."));
    }
    Ok(img)
}
