//! Lecture en lecture seule des systèmes de fichiers exFAT et FAT32 d'une carte ou d'une image.
//! Rien n'est jamais écrit sur la source ici.

use crate::i18n::{f, l};
use crate::source::Source;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Kind {
    #[serde(rename = "exFAT")]
    ExFat,
    #[serde(rename = "FAT32")]
    Fat32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Volume {
    pub id: usize,
    pub kind: Kind,
    pub offset: u64,
    pub size: u64,
    pub label: String,
    pub boot: String,
    #[serde(rename = "mainOk")]
    pub main_ok: bool,
    #[serde(rename = "backupOk")]
    pub backup_ok: bool,
    pub origin: String,
    pub cluster: u64,
    pub notes: Vec<String>,

    #[serde(skip)]
    bps: u64,
    #[serde(skip)]
    cs: u64,
    #[serde(skip)]
    fat_off: u64,
    #[serde(skip)]
    heap_off: u64,
    #[serde(skip)]
    clusters: u32,
    #[serde(skip)]
    root: u32,
    #[serde(skip)]
    media: u8,
    #[serde(skip)]
    backup_sector: u64,
    #[serde(skip)]
    fat: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Fichier {
    pub id: usize,
    pub vol: usize,
    pub name: String,
    pub path: String,
    pub dir: bool,
    pub size: u64,
    pub deleted: bool,
    pub orphan: bool,
    pub mtime: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub warn: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub copied: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub dest: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub duree: f64,
    #[serde(skip)]
    first: u32,
    #[serde(skip)]
    no_fat: bool,
    /// (décalage absolu dans la source, longueur)
    #[serde(skip)]
    pub runs: Vec<(u64, u64)>,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

// ---------- détection des volumes ----------

fn parse_exfat(b: &[u8]) -> Option<Volume> {
    if b.len() < 512 || &b[3..11] != b"EXFAT   " {
        return None;
    }
    let (bps_sh, spc_sh) = (b[108] as u32, b[109] as u32);
    if !(9..=12).contains(&bps_sh) || spc_sh > 25 - bps_sh {
        return None;
    }
    let bps = 1u64 << bps_sh;
    let cs = bps << spc_sh;
    let v = Volume {
        id: 0,
        kind: Kind::ExFat,
        offset: 0,
        size: u64le(b, 72).saturating_mul(bps),
        label: String::new(),
        boot: String::new(),
        main_ok: false,
        backup_ok: false,
        origin: String::new(),
        cluster: cs,
        notes: vec![],
        bps,
        cs,
        fat_off: u32le(b, 80) as u64 * bps,
        heap_off: u32le(b, 88) as u64 * bps,
        clusters: u32le(b, 92),
        root: u32le(b, 96),
        media: 0,
        backup_sector: 12,
        fat: vec![],
    };
    if v.fat_off < 24 * bps || v.clusters == 0 || v.root < 2 || v.root > v.clusters + 1 || v.heap_off <= v.fat_off {
        return None;
    }
    Some(v)
}

fn parse_fat32(b: &[u8]) -> Option<Volume> {
    if b.len() < 512 || b[510] != 0x55 || b[511] != 0xAA {
        return None;
    }
    let bps = u16le(b, 11) as u64;
    let spc = b[13] as u64;
    let res = u16le(b, 14) as u64;
    let nf = b[16] as u64;
    let fat16 = u16le(b, 22);
    let fat_sz = u32le(b, 36) as u64;
    if fat16 != 0 || fat_sz == 0 || ![512, 1024, 2048, 4096].contains(&bps) || spc == 0 || spc & (spc - 1) != 0 || res == 0 || !(1..=2).contains(&nf) {
        return None;
    }
    let mut tot = u32le(b, 32) as u64;
    if tot == 0 {
        tot = u16le(b, 19) as u64;
    }
    let cs = bps * spc;
    let fat_off = res * bps;
    let heap_off = fat_off + nf * fat_sz * bps;
    let size = tot * bps;
    let mut backup = u16le(b, 50) as u64;
    if backup == 0 || backup > res {
        backup = 6;
    }
    let clusters = if size > heap_off { ((size - heap_off) / cs) as u32 } else { 0 };
    let root = u32le(b, 44);
    if clusters < 2 || root < 2 {
        return None;
    }
    let lbl = String::from_utf8_lossy(&b[71..82]).trim().to_string();
    Some(Volume {
        id: 0,
        kind: Kind::Fat32,
        offset: 0,
        size,
        label: if lbl == "NO NAME" { String::new() } else { lbl },
        boot: String::new(),
        main_ok: false,
        backup_ok: false,
        origin: String::new(),
        cluster: cs,
        notes: vec![],
        bps,
        cs,
        fat_off,
        heap_off,
        clusters,
        root,
        media: b[21],
        backup_sector: backup,
        fat: vec![],
    })
}

fn vbr(b: &[u8]) -> Option<Volume> {
    parse_exfat(b).or_else(|| parse_fat32(b))
}

/// Vérifie qu'un volume supposé commencer à `base` est cohérent (dossier racine ou FAT).
fn valider(r: &dyn Source, base: u64, v: &Volume) -> bool {
    match v.kind {
        Kind::ExFat => {
            let root = r.lire(base + v.heap_off + (v.root as u64 - 2) * v.cs, v.cs.min(64 * 1024) as usize);
            let (mut bitmap, mut upcase) = (false, false);
            for e in root.chunks_exact(32) {
                match e[0] {
                    0x81 => bitmap = true,
                    0x82 => upcase = true,
                    0x00 => break,
                    _ => {}
                }
            }
            bitmap && upcase
        }
        Kind::Fat32 => {
            let f = r.lire(base + v.fat_off, 8);
            f.len() == 8 && f[0] == v.media && f[1] == 0xFF && f[2] == 0xFF
        }
    }
}

/// Examine un emplacement : secteur principal, puis copie de secours.
pub fn examiner(r: &dyn Source, base: u64, origine: &str) -> Option<Volume> {
    let tete = r.lire(base, 32 * 4096);
    if tete.len() < 512 {
        return None;
    }
    let mut main = vbr(&tete[..512]);
    if let Some(m) = &main {
        if !valider(r, base, m) {
            main = None;
        }
    }
    let mut backup = None;
    'b: for bps in [512u64, 4096] {
        for sec in [12u64, 6] {
            let off = (sec * bps) as usize;
            if off + 512 > tete.len() {
                continue;
            }
            if let Some(b) = vbr(&tete[off..off + 512]) {
                let bon_secteur = (b.kind == Kind::ExFat && sec == 12) || (b.kind == Kind::Fat32 && sec == b.backup_sector);
                if b.bps == bps && bon_secteur && valider(r, base, &b) {
                    backup = Some(b);
                    break 'b;
                }
            }
        }
    }
    let (main_ok, backup_ok) = (main.is_some(), backup.is_some());
    let mut v = if let Some(m) = main {
        let mut m = m;
        m.boot = l("principal");
        m
    } else if let Some(b) = backup {
        let mut b = b;
        b.boot = l("secours");
        b.notes.push(l("Secteur de démarrage principal abîmé : la copie de secours est utilisée."));
        b
    } else {
        return None;
    };
    v.main_ok = main_ok;
    v.backup_ok = backup_ok;
    v.offset = base;
    v.origin = origine.to_string();
    if main_ok && !backup_ok {
        v.notes.push(l("Copie de secours du secteur de démarrage absente ou abîmée."));
    }
    Some(v)
}

/// Cherche les volumes exFAT / FAT32 : début du support, table MBR ou GPT,
/// puis recherche dans le premier Go si rien n'est trouvé.
pub fn trouver_volumes(r: &dyn Source, stop: &AtomicBool, log: &mut dyn FnMut(String)) -> Vec<Volume> {
    let taille = r.taille();
    let mut vols: Vec<Volume> = vec![];
    let mut vus: HashSet<u64> = HashSet::new();
    let mut ajouter = |v: Option<Volume>, vols: &mut Vec<Volume>| {
        if let Some(mut v) = v {
            if vus.insert(v.offset) {
                v.id = vols.len();
                vols.push(v);
            }
        }
    };
    ajouter(examiner(r, 0, &l("début du support")), &mut vols);

    let s0 = r.lire(0, 512);
    if vols.is_empty() && s0.len() == 512 && s0[510] == 0x55 && s0[511] == 0xAA {
        let mut gpt = false;
        for i in 0..4 {
            let e = &s0[446 + i * 16..446 + i * 16 + 16];
            let (typ, lba) = (e[4], u32le(e, 8) as u64);
            if typ == 0xEE {
                gpt = true;
            }
            if typ != 0 && typ != 0xEE && lba > 0 && lba * 512 < taille {
                ajouter(examiner(r, lba * 512, &l("table des partitions (MBR)")), &mut vols);
            }
        }
        if gpt {
            for bps in [512u64, 4096] {
                let h = r.lire(bps, 512);
                if h.len() < 92 || &h[..8] != b"EFI PART" {
                    continue;
                }
                let (elba, n, sz) = (u64le(&h, 72), u32le(&h, 80) as u64, u32le(&h, 84) as u64);
                if n > 256 || sz < 128 {
                    break;
                }
                let ents = r.lire(elba * bps, (n * sz) as usize);
                let mut i = 0u64;
                while (i + 1) * sz <= ents.len() as u64 {
                    let first = u64le(&ents, (i * sz + 32) as usize);
                    if first > 0 && first * bps < taille {
                        ajouter(examiner(r, first * bps, &l("table des partitions (GPT)")), &mut vols);
                    }
                    i += 1;
                }
                break;
            }
        }
    }

    if vols.is_empty() {
        let limite = taille.min(1 << 30);
        log(f("Aucun volume reconnu au début du support : recherche dans les {} premiers...", &[&crate::fmt_bytes(limite)]));
        const BLOC: u64 = 4 << 20;
        let mut off = 0u64;
        'scan: while off < limite && vols.is_empty() {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let b = r.lire(off, (BLOC + 512) as usize);
            let mut i = 0usize;
            while i + 512 <= b.len() && (i as u64) < BLOC {
                if b[i + 3] == b'E' || b[i + 82] == b'F' {
                    if let Some(v) = vbr(&b[i..i + 512]) {
                        let p = off + i as u64;
                        let mut bases = vec![p];
                        if p >= 12 * v.bps {
                            bases.push(p - 12 * v.bps);
                        }
                        if p >= v.backup_sector * v.bps {
                            bases.push(p - v.backup_sector * v.bps);
                        }
                        for base in bases {
                            if valider(r, base, &v) {
                                let w = examiner(r, base, &l("recherche sur le support")).unwrap_or_else(|| {
                                    let mut w = v.clone();
                                    w.offset = base;
                                    w.boot = l("secours");
                                    w.origin = l("recherche sur le support");
                                    w
                                });
                                ajouter(Some(w), &mut vols);
                                break 'scan;
                            }
                        }
                    }
                }
                i += 512;
            }
            off += BLOC;
        }
    }
    vols
}

// ---------- FAT ----------

impl Volume {
    fn charger_fat(&mut self, r: &dyn Source) {
        if !self.fat.is_empty() {
            return;
        }
        let mut n = self.clusters as u64 + 2;
        if n * 4 > 512 << 20 {
            n = (512 << 20) / 4;
        }
        let b = r.lire(self.offset + self.fat_off, (n * 4) as usize);
        self.fat = b
            .chunks_exact(4)
            .map(|c| {
                let x = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                if self.kind == Kind::Fat32 {
                    x & 0x0FFF_FFFF
                } else {
                    x
                }
            })
            .collect();
    }

    fn fin_chaine(&self, c: u32) -> bool {
        match self.kind {
            Kind::Fat32 => c >= 0x0FFF_FFF8 || c < 2,
            Kind::ExFat => c >= 0xFFFF_FFF7 || c < 2,
        }
    }

    fn pos_cluster(&self, c: u32) -> u64 {
        self.offset + self.heap_off + (c as u64 - 2) * self.cs
    }

    /// Liste des clusters d'une chaîne (None si la chaîne est invalide).
    fn chaine(&self, first: u32, max: usize) -> Option<Vec<u32>> {
        let mut l = vec![];
        let mut vus = HashSet::new();
        let mut c = first;
        while !self.fin_chaine(c) {
            if c as usize >= self.fat.len() || !vus.insert(c) || c > self.clusters + 1 || l.len() > max {
                return None;
            }
            l.push(c);
            c = self.fat[c as usize];
        }
        Some(l)
    }

    fn runs_de(&self, clusters: &[u32]) -> Vec<(u64, u64)> {
        let mut runs: Vec<(u64, u64)> = vec![];
        for &c in clusters {
            let p = self.pos_cluster(c);
            match runs.last_mut() {
                Some(r) if r.0 + r.1 == p => r.1 += self.cs,
                _ => runs.push((p, self.cs)),
            }
        }
        runs
    }

    /// Calcule l'emplacement des données d'un fichier.
    fn placer(&self, f: &mut Fichier, taille: u64) {
        if f.first < 2 || f.first > self.clusters + 1 {
            if taille > 0 {
                f.warn = l("Emplacement des données inconnu");
            }
            return;
        }
        let nb = ((taille + self.cs - 1) / self.cs) as usize;
        if !f.no_fat {
            if let Some(ch) = self.chaine(f.first, nb + 8) {
                // après une suppression, la FAT peut avoir été conservée : on l'accepte si sa longueur colle
                if (!f.deleted && ch.len() >= nb) || (f.deleted && ch.len() == nb) {
                    f.runs = tronquer(self.runs_de(&ch), taille);
                    return;
                }
            }
        }
        let debut = self.pos_cluster(f.first);
        let fin = self.offset + self.heap_off + self.clusters as u64 * self.cs;
        let mut taille = taille;
        if debut + taille > fin {
            taille = fin.saturating_sub(debut);
            f.warn = l("Fichier tronqué à la fin du volume");
        }
        if !f.no_fat {
            f.warn = l("Données supposées contiguës, le fichier peut être incomplet");
        }
        f.runs = vec![(debut, taille)];
    }

    fn lire_runs(&self, r: &dyn Source, runs: &[(u64, u64)], max: u64) -> Vec<u8> {
        let mut b = vec![];
        for &(p, n) in runs {
            let n = n.min(max - b.len() as u64);
            if n == 0 {
                break;
            }
            b.extend(r.lire(p, n as usize));
        }
        b
    }
}

fn tronquer(runs: Vec<(u64, u64)>, mut taille: u64) -> Vec<(u64, u64)> {
    let mut res = vec![];
    for (p, n) in runs {
        if taille == 0 {
            break;
        }
        let n = n.min(taille);
        res.push((p, n));
        taille -= n;
    }
    res
}

// ---------- horodatage ----------

fn date_dos(ts: u32) -> i64 {
    use chrono::{Local, NaiveDate, TimeZone};
    let (d, t) = (ts >> 16, ts & 0xFFFF);
    let (y, mo, da) = ((d >> 9) as i32 + 1980, (d >> 5) & 15, d & 31);
    let (h, mi, s) = (t >> 11, (t >> 5) & 63, (t & 31) * 2);
    NaiveDate::from_ymd_opt(y, mo, da)
        .and_then(|dt| dt.and_hms_opt(h, mi, s.min(59)))
        .and_then(|n| Local.from_local_datetime(&n).earliest())
        .map(|x| x.timestamp())
        .unwrap_or(0)
}

// ---------- exFAT ----------

fn somme_exfat(set: &[u8]) -> u16 {
    let mut s: u16 = 0;
    for (i, &c) in set.iter().enumerate() {
        if i == 2 || i == 3 {
            continue;
        }
        // les entrées supprimées ont perdu ce bit après le calcul de la somme
        let c = if i % 32 == 0 { c | 0x80 } else { c };
        s = (s << 15 | s >> 1).wrapping_add(c as u16);
    }
    s
}

/// Lit un ensemble d'entrées exFAT commençant en b[i] ; renvoie le fichier et le nombre d'entrées.
fn ensemble_exfat(b: &[u8], i: usize) -> Option<(Fichier, usize)> {
    let t = b[i];
    if t != 0x85 && t != 0x05 {
        return None;
    }
    let sc = b[i + 1] as usize;
    if !(2..=18).contains(&sc) || i + (sc + 1) * 32 > b.len() {
        return None;
    }
    let set = &b[i..i + (sc + 1) * 32];
    let st = set[32];
    if st & 0x7F != 0x40 || u16le(set, 2) != somme_exfat(set) {
        return None;
    }
    let nlen = set[32 + 3] as usize;
    let mut nom: Vec<u16> = vec![];
    for k in 2..=sc {
        if nom.len() >= nlen {
            break;
        }
        let e = &set[k * 32..];
        if e[0] & 0x7F != 0x41 {
            return None;
        }
        for j in 0..15 {
            if nom.len() >= nlen {
                break;
            }
            nom.push(u16le(e, 2 + j * 2));
        }
    }
    let name = String::from_utf16_lossy(&nom);
    if name.is_empty() || name.contains(['\0', '/', '\\']) {
        return None;
    }
    Some((
        Fichier {
            name,
            dir: u16le(set, 4) & 0x10 != 0,
            deleted: t == 0x05 || st & 0x80 == 0,
            mtime: date_dos(u32le(set, 12)),
            first: u32le(set, 32 + 20),
            no_fat: set[32 + 1] & 0x02 != 0,
            size: u64le(set, 32 + 24),
            ..Default::default()
        },
        sc + 1,
    ))
}

impl Volume {
    fn lire_dossier_exfat(&mut self, r: &dyn Source, data: &[u8], chemin: &str, supprime: bool, prof: u32, out: &mut Vec<Fichier>, vus_dir: &mut HashSet<u32>) {
        let mut i = 0usize;
        while i + 32 <= data.len() {
            let t = data[i];
            if t == 0x00 {
                // fin des entrées en usage ; des entrées supprimées peuvent suivre dans un ancien dossier
                i += 32;
                continue;
            }
            if t == 0x83 && chemin.is_empty() {
                let n = (data[i + 1] as usize).min(11);
                let nom: Vec<u16> = (0..n).map(|j| u16le(data, i + 2 + j * 2)).collect();
                self.label = String::from_utf16_lossy(&nom);
            }
            let Some((mut f, n)) = ensemble_exfat(data, i) else {
                i += 32;
                continue;
            };
            i += n * 32;
            f.path = chemin.to_string();
            f.deleted = f.deleted || supprime;
            f.vol = self.id;
            if f.dir {
                let (first, sz, del, name) = (f.first, f.size, f.deleted, f.name.clone());
                let mut g = f.clone();
                out.push(f);
                if prof < 32 && first >= 2 && vus_dir.insert(first) {
                    let taille = if sz == 0 || sz > 64 << 20 { self.cs } else { sz };
                    self.placer(&mut g, taille);
                    let sub = self.lire_runs(r, &g.runs, 64 << 20);
                    let p = joindre(chemin, &name);
                    self.lire_dossier_exfat(r, &sub, &p, del, prof + 1, out, vus_dir);
                }
                continue;
            }
            let sz = f.size;
            self.placer(&mut f, sz);
            out.push(f);
        }
    }
}

fn joindre(a: &str, b: &str) -> String {
    if a.is_empty() {
        b.to_string()
    } else {
        format!("{a}/{b}")
    }
}

// ---------- FAT32 ----------

fn nom_court(e: &[u8]) -> String {
    let mut base: Vec<u8> = e[0..8].to_vec();
    while base.last() == Some(&b' ') {
        base.pop();
    }
    let mut ext: Vec<u8> = e[8..11].to_vec();
    while ext.last() == Some(&b' ') {
        ext.pop();
    }
    if base.first() == Some(&0x05) {
        base[0] = 0xE5;
    }
    let base: String = base.iter().map(|&c| c as char).collect();
    let ext: String = ext.iter().map(|&c| c as char).collect();
    if ext.is_empty() {
        base
    } else {
        format!("{base}.{ext}")
    }
}

/// FAT efface la première lettre des fichiers supprimés : on la déduit des fichiers voisins
/// de même longueur et même extension (P1000002.MOV à côté de P1000001.MOV).
fn deviner_initiale(out: &mut [Fichier], cibles: &[usize], voisins: &[usize]) {
    let ext = |s: &str| s.rsplit_once('.').map(|x| x.1.to_lowercase()).unwrap_or_default();
    for &c in cibles {
        let (len, e) = (out[c].name.chars().count(), ext(&out[c].name));
        let mut compte: HashMap<char, usize> = HashMap::new();
        for &v in voisins {
            let o = &out[v].name;
            if o.chars().count() == len && ext(o) == e && !o.starts_with('_') {
                *compte.entry(o.chars().next().unwrap()).or_default() += 1;
            }
        }
        if compte.len() == 1 {
            let ch = *compte.keys().next().unwrap();
            let reste: String = out[c].name.chars().skip(1).collect();
            out[c].name = format!("{ch}{reste}");
            out[c].warn = format!("{}{}", out[c].warn, l(" Première lettre du nom déduite des fichiers voisins.")).trim().to_string();
        }
    }
}

impl Volume {
    fn lire_dossier_fat(&mut self, r: &dyn Source, data: &[u8], chemin: &str, supprime: bool, prof: u32, out: &mut Vec<Fichier>, vus_dir: &mut HashSet<u32>) {
        let mut a_deviner: Vec<usize> = vec![];
        let mut voisins: Vec<usize> = vec![];
        // parties du nom long, dans l'ordre du disque : la dernière partie du nom en premier
        let mut lfn: Vec<Vec<u16>> = vec![];
        for e in data.chunks_exact(32) {
            if e[0] == 0x00 {
                lfn.clear();
                continue;
            }
            let attr = e[11];
            let del = e[0] == 0xE5;
            if attr == 0x0F {
                let mut p = vec![];
                'parts: for (o, n) in [(1usize, 5usize), (14, 6), (28, 2)] {
                    for j in 0..n {
                        let c = u16le(e, o + j * 2);
                        if c == 0 || c == 0xFFFF {
                            break 'parts;
                        }
                        p.push(c);
                    }
                }
                lfn.push(p);
                continue;
            }
            if attr & 0x08 != 0 {
                if chemin.is_empty() && !del && attr & 0x10 == 0 {
                    self.label = nom_court(e).trim().to_string();
                }
                lfn.clear();
                continue;
            }
            let mut nom = nom_court(e);
            let sans_lfn = lfn.is_empty();
            if !sans_lfn {
                let long: Vec<u16> = lfn.iter().rev().flatten().copied().collect();
                let s = String::from_utf16_lossy(&long);
                if !s.is_empty() {
                    nom = s;
                }
            } else if del && !nom.is_empty() {
                nom = format!("_{}", nom.chars().skip(1).collect::<String>());
            }
            let devine = del && sans_lfn && nom.starts_with('_');
            lfn.clear();
            if nom == "." || nom == ".." || nom.trim().is_empty() {
                continue;
            }
            let first = (u16le(e, 20) as u32) << 16 | u16le(e, 26) as u32;
            let mut f = Fichier {
                name: nom.clone(),
                path: chemin.to_string(),
                vol: self.id,
                dir: attr & 0x10 != 0,
                deleted: del || supprime,
                mtime: date_dos((u16le(e, 24) as u32) << 16 | u16le(e, 22) as u32),
                first,
                size: u32le(e, 28) as u64,
                ..Default::default()
            };
            if f.dir {
                let d = f.deleted;
                out.push(f);
                if prof < 32 && first >= 2 && vus_dir.insert(first) {
                    let sub = self.lire_dossier_chaine(r, first, d);
                    let p = joindre(chemin, &nom);
                    self.lire_dossier_fat(r, &sub, &p, d, prof + 1, out, vus_dir);
                }
                continue;
            }
            let sz = f.size;
            self.placer(&mut f, sz);
            out.push(f);
            if devine {
                a_deviner.push(out.len() - 1);
            } else {
                voisins.push(out.len() - 1);
            }
        }
        deviner_initiale(out, &a_deviner, &voisins);
    }

    /// Lit un dossier FAT32 : chaîne FAT si possible, sinon clusters contigus jusqu'à la fin des entrées.
    fn lire_dossier_chaine(&self, r: &dyn Source, first: u32, supprime: bool) -> Vec<u8> {
        if !supprime {
            if let Some(ch) = self.chaine(first, 4096) {
                return self.lire_runs(r, &self.runs_de(&ch), 64 << 20);
            }
        }
        let mut b = vec![];
        for k in 0..64u32 {
            let c = first + k;
            if c > self.clusters + 1 {
                break;
            }
            let d = r.lire(self.pos_cluster(c), self.cs as usize);
            let fini = d.chunks_exact(32).any(|e| e[0] == 0);
            b.extend(d);
            if fini {
                break;
            }
        }
        b
    }

    // ---------- lecture complète ----------

    pub fn lire(&mut self, r: &dyn Source) -> Vec<Fichier> {
        self.charger_fat(r);
        let mut out = vec![];
        let mut vus_dir: HashSet<u32> = [self.root].into();
        let root = match self.chaine(self.root, 1 << 16) {
            Some(ch) => self.lire_runs(r, &self.runs_de(&ch), 64 << 20),
            None => {
                self.notes.push(l("Table d'allocation abîmée : seul le début du dossier racine a pu être lu."));
                r.lire(self.pos_cluster(self.root), self.cs as usize)
            }
        };
        match self.kind {
            Kind::ExFat => self.lire_dossier_exfat(r, &root, "", false, 0, &mut out, &mut vus_dir),
            Kind::Fat32 => self.lire_dossier_fat(r, &root, "", false, 0, &mut out, &mut vus_dir),
        }
        out
    }

    /// Parcourt toute la zone de données à la recherche d'anciennes entrées de fichiers
    /// (après un formatage rapide par exemple).
    pub fn recherche_approfondie(&mut self, r: &dyn Source, connus: &[Fichier], stop: &AtomicBool, prog: &mut dyn FnMut(u64, u64)) -> Vec<Fichier> {
        let cle = |f: &Fichier| format!("{}/{}/{}", f.first, f.size, f.name.to_lowercase());
        let mut deja: HashSet<String> = connus.iter().map(cle).collect();
        let mut out: Vec<Fichier> = vec![];
        let debut = self.offset + self.heap_off;
        let total = self.clusters as u64 * self.cs;
        const BLOC: u64 = 8 << 20;
        let recouvre: u64 = if self.kind == Kind::ExFat { 19 * 32 } else { 0 };
        let mut vus_dir: HashSet<u32> = HashSet::new();
        let mut off = 0u64;
        while off < total {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            prog(off, total);
            let n = (BLOC + recouvre).min(total - off);
            let b = r.lire(debut + off, n as usize);
            match self.kind {
                Kind::ExFat => {
                    let mut i = 0usize;
                    while i + 64 <= b.len() && (i as u64) < BLOC {
                        if b[i] == 0x85 || b[i] == 0x05 {
                            if let Some((mut f, k)) = ensemble_exfat(&b, i) {
                                if !f.dir {
                                    f.vol = self.id;
                                    f.orphan = true;
                                    f.deleted = true;
                                    f.path = "_orphelins".into();
                                    if deja.insert(cle(&f)) {
                                        let sz = f.size;
                                        self.placer(&mut f, sz);
                                        out.push(f);
                                    }
                                    i += k * 32;
                                    continue;
                                }
                            }
                        }
                        i += 32;
                    }
                }
                Kind::Fat32 => {
                    // un ancien sous-dossier commence par les entrées "." et ".."
                    let mut i = 0u64;
                    while i + 64 <= b.len() as u64 {
                        let e = &b[i as usize..];
                        if &e[0..11] == b".          " && &e[32..43] == b"..         " && e[11] & 0x10 != 0 {
                            let c = ((off + i) / self.cs) as u32 + 2;
                            if vus_dir.insert(c) {
                                let mut trouves = vec![];
                                let data = self.lire_dossier_chaine(r, c, true);
                                let mut v2: HashSet<u32> = [c].into();
                                self.lire_dossier_fat(r, &data, &format!("_orphelins/dossier_{c}"), true, 30, &mut trouves, &mut v2);
                                for mut f in trouves {
                                    if f.dir || !deja.insert(cle(&f)) {
                                        continue;
                                    }
                                    f.orphan = true;
                                    out.push(f);
                                }
                            }
                        }
                        i += self.cs;
                    }
                }
            }
            off += BLOC;
        }
        prog(total, total);
        out
    }
}

pub fn trier_fichiers(l: &mut [Fichier]) {
    l.sort_by(|a, b| {
        a.vol
            .cmp(&b.vol)
            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Renvoie (décalage, données) à écrire pour remettre le secteur de démarrage principal
/// à partir de la copie de secours.
pub fn region_restauration(r: &dyn Source, v: &Volume) -> Option<(u64, Vec<u8>)> {
    if !v.backup_ok {
        return None;
    }
    let (sec, n) = match v.kind {
        Kind::ExFat => (12, 12),
        Kind::Fat32 => (v.backup_sector, 3),
    };
    let d = r.lire(v.offset + sec * v.bps, (n * v.bps) as usize);
    if d.len() as u64 != n * v.bps {
        return None;
    }
    Some((v.offset, d))
}
