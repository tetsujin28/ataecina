//! Ataecina : moteur de récupération et de réparation de rushes.
//!
//! - `fsread`   : lecture exFAT / FAT32 en lecture seule, fichiers supprimés, recherche après formatage
//! - `imager`   : image de carte selon la méthode de GNU ddrescue, avec reprise
//! - `analyse`  : analyse d'une carte, copie des fichiers, restauration du secteur de démarrage
//! - `photorec` : pilotage de PhotoRec
//! - `repair`   : réparation des MOV / MP4 avec untrunc
//! - `platform` : lecteurs, droits administrateur, écriture de secteurs (Windows)

pub mod analyse;
pub mod fsread;
pub mod i18n;
pub mod imager;
pub mod photorec;
pub mod platform;
pub mod repair;
pub mod source;
pub mod tools;

use std::path::Path;

/// Taille lisible selon la langue : « 6,5 Mo » en français, « 6.5 MB » en anglais, « 6,5 MB » en espagnol.
pub fn fmt_bytes(b: u64) -> String {
    let lang = i18n::lang();
    let u: [&str; 5] = if lang == "fr" { ["o", "Ko", "Mo", "Go", "To"] } else { ["B", "KB", "MB", "GB", "TB"] };
    let mut f = b as f64;
    let mut i = 0;
    while f >= 1024.0 && i < u.len() - 1 {
        f /= 1024.0;
        i += 1;
    }
    if i == 0 {
        return format!("{b} {}", u[0]);
    }
    let s = format!("{f:.1} {}", u[i]);
    if lang == "en" {
        s
    } else {
        s.replacen('.', ",", 1)
    }
}

/// Nombre décimal selon la langue (virgule en français et en espagnol).
pub fn fmt_dec(x: f64) -> String {
    let s = format!("{x:.1}");
    if i18n::lang() == "en" {
        s
    } else {
        s.replace('.', ",")
    }
}

/// Durée « 1m23s » à la manière de Go.
pub fn fmt_duree(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h{}m{}s", s / 3600, s % 3600 / 60, s % 60)
    } else if s >= 60 {
        format!("{}m{}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

/// Remplace les caractères interdits dans un nom de fichier Windows.
pub fn nom_sur(s: &str) -> String {
    s.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect()
}

/// Vrai si le fichier existe et n'est pas un dossier.
pub fn existe_fichier(p: &Path) -> bool {
    p.metadata().map(|m| m.is_file()).unwrap_or(false)
}
