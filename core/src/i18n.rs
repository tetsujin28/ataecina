//! Traductions des messages du moteur (journal, erreurs). Le texte français sert de clé.
//! Les clés absentes restent en français.

use std::sync::atomic::{AtomicU8, Ordering};

static LANG: AtomicU8 = AtomicU8::new(0);

/// Langue des messages : "fr", "en" ou "es".
pub fn set_lang(l: &str) {
    LANG.store(match l { "en" => 1, "es" => 2, _ => 0 }, Ordering::Relaxed);
}

pub fn lang() -> &'static str {
    match LANG.load(Ordering::Relaxed) { 1 => "en", 2 => "es", _ => "fr" }
}

/// Traduit un message français dans la langue courante.
pub fn l(fr: &str) -> String {
    let i = LANG.load(Ordering::Relaxed);
    if i == 0 {
        return fr.to_string();
    }
    let t: Option<[&str; 2]> = match fr {
        "Impossible de créer le dossier de sortie : " => Some(["Cannot create the output folder: ", "No se puede crear la carpeta de salida: "]),
        "   déjà lisible ({} s), copié sans réparation" => Some(["   already readable ({} s), copied without repair", "   ya legible ({} s), copiado sin reparar"]),
        "   {} : {} s récupérées (référence {}, mode {})" => Some(["   {}: {} s recovered (reference {}, mode {})", "   {}: {} s recuperados (referencia {}, modo {})"]),
        ", {} erreur(s) de décodage" => Some([", {} decoding error(s)", ", {} error(es) de decodificación"]),
        "   annulé" => Some(["   cancelled", "   cancelado"]),
        "   ÉCHEC : aucune référence n'a permis de réparer ce fichier" => Some(["   FAILED: no reference could repair this file", "   FALLO: ninguna referencia ha permitido reparar este archivo"]),
        "Terminé en {} : {} OK, {} intacts, {} avec erreurs, {} échecs{}" => Some(["Finished in {}: {} OK, {} intact, {} with errors, {} failed{}", "Terminado en {}: {} OK, {} intactos, {} con errores, {} fallidos{}"]),
        ", {} annulés" => Some([", {} cancelled", ", {} cancelados"]),
        "Journal : " => Some(["Log: ", "Registro: "]),
        "   lisible, vérification complète..." => Some(["   readable, full check...", "   legible, verificación completa..."]),
        "   copie impossible : " => Some(["   copy failed: ", "   copia imposible: "]),
        "   untrunc {} avec {}..." => Some(["   untrunc {} with {}...", "   untrunc {} con {}..."]),
        "   résultat {} s, vérification complète..." => Some(["   result {} s, full check...", "   resultado {} s, verificación completa..."]),
        "   déplacement impossible : " => Some(["   move failed: ", "   no se puede mover: "]),
        "Une récupération est déjà en cours." => Some(["A recovery is already running.", "Ya hay una recuperación en curso."]),
        "Choisissez un dossier de destination pour les fichiers récupérés." => Some(["Choose a destination folder for the recovered files.", "Elige una carpeta de destino para los archivos recuperados."]),
        "Choisissez la carte ou le disque à analyser." => Some(["Choose the card or drive to analyse.", "Elige la tarjeta o el disco que quieres analizar."]),
        "La destination ne doit pas être sur la carte analysée : on risquerait d'écraser ce qu'on cherche à récupérer." => Some(["The destination must not be on the card being analysed: it could overwrite what you are trying to recover.", "El destino no debe estar en la tarjeta analizada: podría sobrescribir lo que intentas recuperar."]),
        "Fichier image introuvable." => Some(["Image file not found.", "No se encuentra el archivo de imagen."]),
        "PhotoRec introuvable. Indiquez l'emplacement de photorec_win.exe (fourni avec TestDisk), ou activez la création d'image seule." => Some(["PhotoRec not found. Point to photorec_win.exe (included with TestDisk), or enable image creation only.", "No se encuentra PhotoRec. Indica dónde está photorec_win.exe (incluido con TestDisk), o activa solo la creación de la imagen."]),
        "L'image ne peut pas être enregistrée sur la carte elle-même." => Some(["The image cannot be saved on the card itself.", "La imagen no se puede guardar en la propia tarjeta."]),
        "Pas assez de place pour l'image : il faut {}, il reste {} sur la destination." => Some(["Not enough space for the image: {} needed, {} left on the destination.", "No hay espacio suficiente para la imagen: se necesitan {}, quedan {} en el destino."]),
        "Récupération arrêtée." => Some(["Recovery stopped.", "Recuperación detenida."]),
        "ERREUR : " => Some(["ERROR: ", "ERROR: "]),
        "{} fichier(s) MOV / MP4 ajouté(s) à la liste de réparation." => Some(["{} MOV / MP4 file(s) added to the repair list.", "{} archivo(s) MOV / MP4 añadido(s) a la lista de reparación."]),
        "{} fichier(s) RAW (R3D, ARRIRAW, MXF, BRAW) récupéré(s) : ils ne passent pas par untrunc." => Some(["{} RAW file(s) (R3D, ARRIRAW, MXF, BRAW) recovered: they are not processed by untrunc.", "{} archivo(s) RAW (R3D, ARRIRAW, MXF, BRAW) recuperado(s): no pasan por untrunc."]),
        "Source : {} ({})" => Some(["Source: {} ({})", "Origen: {} ({})"]),
        "Source : image " => Some(["Source: image ", "Origen: imagen "]),
        "PhotoRec n'est pas installé : l'image est prête, l'analyse n'a pas été lancée." => Some(["PhotoRec is not installed: the image is ready, the analysis was not started.", "PhotoRec no está instalado: la imagen está lista, el análisis no se ha iniciado."]),
        "Analyse de la carte avec PhotoRec : " => Some(["Analysing the card with PhotoRec: ", "Análisis de la tarjeta con PhotoRec: "]),
        "impossible de lancer PhotoRec : {}" => Some(["cannot start PhotoRec: {}", "no se puede iniciar PhotoRec: {}"]),
        "   trouvé {} ({})" => Some(["   found {} ({})", "   encontrado {} ({})"]),
        "PhotoRec s'est arrêté avec une erreur ({}). Consultez photorec.log dans {}" => Some(["PhotoRec stopped with an error ({}). See photorec.log in {}", "PhotoRec se ha detenido con un error ({}). Consulta photorec.log en {}"]),
        "Terminé : {} fichier(s) vidéo récupéré(s), {}." => Some(["Finished: {} video file(s) recovered, {}.", "Terminado: {} archivo(s) de vídeo recuperado(s), {}."]),
        "Lire une carte directement demande les droits administrateur." => Some(["Reading a card directly requires administrator rights.", "Leer una tarjeta directamente requiere permisos de administrador."]),
        "Arrêt demandé..." => Some(["Stop requested...", "Detención solicitada..."]),
        "Relance en administrateur refusée ou impossible : " => Some(["Restart as administrator refused or impossible: ", "Reinicio como administrador rechazado o imposible: "]),
        "lecture impossible de {} : {}" => Some(["cannot read {}: {}", "no se puede leer {}: {}"]),
        "taille de la source inconnue" => Some(["unknown source size", "tamaño del origen desconocido"]),
        "Une analyse ou une copie est déjà en cours." => Some(["An analysis or a copy is already running.", "Ya hay un análisis o una copia en curso."]),
        "Choisissez où enregistrer l'image (dossier image ou destination)." => Some(["Choose where to save the image (image folder or destination).", "Elige dónde guardar la imagen (carpeta de imagen o destino)."]),
        "Pas assez de place pour l'image : il faut {}, il reste {}." => Some(["Not enough space for the image: {} needed, {} left.", "No hay espacio suficiente para la imagen: se necesitan {}, quedan {}."]),
        "Analyse arrêtée." => Some(["Analysis stopped.", "Análisis detenido."]),
        "Source : carte {}" => Some(["Source: card {}", "Origen: tarjeta {}"]),
        "Recherche des volumes exFAT / FAT32..." => Some(["Looking for exFAT / FAT32 volumes...", "Buscando volúmenes exFAT / FAT32..."]),
        "Aucun système de fichiers exFAT ou FAT32 n'a été trouvé. La carte est peut-être dans un autre format (NTFS, HFS+, UDF) ou trop abîmée : essayez la méthode PhotoRec." => Some(["No exFAT or FAT32 file system was found. The card may use another format (NTFS, HFS+, UDF) or be too damaged: try the PhotoRec method.", "No se ha encontrado ningún sistema de archivos exFAT o FAT32. La tarjeta puede estar en otro formato (NTFS, HFS+, UDF) o demasiado dañada: prueba el método PhotoRec."]),
        "Volume {} trouvé ({}) : {}, clusters de {}, secteur de démarrage {}." => Some(["{} volume found ({}): {}, {} clusters, {} boot sector.", "Volumen {} encontrado ({}): {}, clústeres de {}, sector de arranque {}."]),
        "principal" => Some(["main", "principal"]),
        "secours" => Some(["backup", "de respaldo"]),
        "Attention : " => Some(["Warning: ", "Atención: "]),
        "Nom du volume : {}" => Some(["Volume name: {}", "Nombre del volumen: {}"]),
        "{} fichier(s) lu(s), dont {} supprimé(s)." => Some(["{} file(s) read, including {} deleted.", "{} archivo(s) leído(s), de ellos {} borrado(s)."]),
        "Recherche approfondie des anciennes entrées de fichiers..." => Some(["Deep search for old file entries...", "Búsqueda profunda de antiguas entradas de archivos..."]),
        "{} fichier(s) supplémentaire(s) retrouvé(s) par la recherche approfondie." => Some(["{} additional file(s) found by the deep search.", "{} archivo(s) adicional(es) encontrado(s) por la búsqueda profunda."]),
        "Terminé : {} fichier(s) dont {} vidéo(s). Sélectionnez ceux à copier." => Some(["Finished: {} file(s), including {} video(s). Select the ones to copy.", "Terminado: {} archivo(s), de ellos {} vídeo(s). Selecciona los que quieres copiar."]),
        "Une opération est déjà en cours." => Some(["An operation is already running.", "Ya hay una operación en curso."]),
        "Lancez d'abord l'analyse." => Some(["Run the analysis first.", "Primero lanza el análisis."]),
        "Aucun fichier sélectionné." => Some(["No file selected.", "Ningún archivo seleccionado."]),
        "Choisissez un dossier de destination." => Some(["Choose a destination folder.", "Elige una carpeta de destino."]),
        "La destination ne doit pas être sur la carte analysée." => Some(["The destination must not be on the card being analysed.", "El destino no debe estar en la tarjeta analizada."]),
        "Pas assez de place : il faut {}, il reste {}." => Some(["Not enough space: {} needed, {} left.", "No hay espacio suficiente: se necesitan {}, quedan {}."]),
        "Copie de {} fichier(s) ({}) vers {}" => Some(["Copying {} file(s) ({}) to {}", "Copiando {} archivo(s) ({}) a {}"]),
        "Copie arrêtée." => Some(["Copy stopped.", "Copia detenida."]),
        "écriture de {} : {}" => Some(["writing {}: {}", "escritura de {}: {}"]),
        "   {} copié, lisible ({} s)" => Some(["   {} copied, readable ({} s)", "   {} copiado, legible ({} s)"]),
        "   {} copié" => Some(["   {} copied", "   {} copiado"]),
        "   {} copié mais {}" => Some(["   {} copied but {}", "   {} copiado pero {}"]),
        "illisible : à réparer" => Some(["unreadable: needs repair", "ilegible: hay que repararlo"]),
        "incomplet" => Some(["incomplete", "incompleto"]),
        "Copie terminée : {} fichier(s) bons, {} à vérifier." => Some(["Copy finished: {} good file(s), {} to check.", "Copia terminada: {} archivo(s) correctos, {} por revisar."]),
        "{} vidéo(s) illisible(s) ajoutée(s) à la liste de réparation." => Some(["{} unreadable video(s) added to the repair list.", "{} vídeo(s) ilegible(s) añadido(s) a la lista de reparación."]),
        "Une opération est en cours." => Some(["An operation is running.", "Hay una operación en curso."]),
        "Volume inconnu." => Some(["Unknown volume.", "Volumen desconocido."]),
        "Le secteur de démarrage principal est déjà correct." => Some(["The main boot sector is already correct.", "El sector de arranque principal ya es correcto."]),
        "Pas de copie de secours utilisable pour ce volume." => Some(["No usable backup copy for this volume.", "No hay copia de respaldo utilizable para este volumen."]),
        "Restauration du secteur de démarrage de {} à partir de la copie de secours..." => Some(["Restoring the boot sector of {} from the backup copy...", "Restaurando el sector de arranque de {} a partir de la copia de respaldo..."]),
        "ERREUR : la vérification après restauration a échoué." => Some(["ERROR: the check after restoring failed.", "ERROR: la verificación tras la restauración ha fallado."]),
        "La restauration a été écrite mais la vérification a échoué." => Some(["The restore was written but the check failed.", "La restauración se ha escrito pero la verificación ha fallado."]),
        "Secteur de démarrage restauré." => Some(["Boot sector restored.", "Sector de arranque restaurado."]),
        "Secteur de démarrage restauré et vérifié." => Some(["Boot sector restored and checked.", "Sector de arranque restaurado y verificado."]),
        " Retirez puis réinsérez la carte : Windows devrait de nouveau la lire." => Some([" Remove and reinsert the card: Windows should be able to read it again.", " Retira y vuelve a insertar la tarjeta: Windows debería poder leerla de nuevo."]),
        "Requête invalide" => Some(["Invalid request", "Solicitud no válida"]),
        "Écrire sur la carte demande les droits administrateur." => Some(["Writing to the card requires administrator rights.", "Escribir en la tarjeta requiere permisos de administrador."]),
        "taille de la source inconnue : {}" => Some(["unknown source size: {}", "tamaño del origen desconocido: {}"]),
        "création de l'image : {}" => Some(["creating the image: {}", "creación de la imagen: {}"]),
        "Copie rapide" => Some(["Fast copy", "Copia rápida"]),
        "Copie inverse" => Some(["Reverse copy", "Copia inversa"]),
        "Rognage" => Some(["Trimming", "Recorte"]),
        "Raclage" => Some(["Scraping", "Raspado"]),
        "Réessai {}/{}" => Some(["Retry {}/{}", "Reintento {}/{}"]),
        "Terminé" => Some(["Finished", "Terminado"]),
        "Reprise de l'image interrompue : " => Some(["Resuming interrupted image: ", "Reanudando la imagen interrumpida: "]),
        "Image de la carte : " => Some(["Card image: ", "Imagen de la tarjeta: "]),
        "Image interrompue : relancez avec la même carte et le même dossier pour la reprendre là où elle s'est arrêtée." => Some(["Image interrupted: run again with the same card and folder to resume where it stopped.", "Imagen interrumpida: vuelve a lanzarla con la misma tarjeta y la misma carpeta para reanudarla donde se detuvo."]),
        "Image terminée : {} en {}" => Some(["Image finished: {} in {}", "Imagen terminada: {} en {}"]),
        "Attention : {} illisibles sur la carte malgré les réessais, remplacés par des zéros (détail dans {}.map)." => Some(["Warning: {} unreadable on the card despite retries, replaced with zeros (details in {}.map).", "Atención: {} ilegibles en la tarjeta pese a los reintentos, sustituidos por ceros (detalle en {}.map)."]),
        "Aucun secteur illisible." => Some(["No unreadable sectors.", "Ningún sector ilegible."]),
        "Secteur de démarrage principal abîmé : la copie de secours est utilisée." => Some(["Main boot sector damaged: using the backup copy.", "Sector de arranque principal dañado: se usa la copia de respaldo."]),
        "Copie de secours du secteur de démarrage absente ou abîmée." => Some(["Backup copy of the boot sector missing or damaged.", "Copia de respaldo del sector de arranque ausente o dañada."]),
        "début du support" => Some(["start of the media", "inicio del soporte"]),
        "table des partitions (MBR)" => Some(["partition table (MBR)", "tabla de particiones (MBR)"]),
        "table des partitions (GPT)" => Some(["partition table (GPT)", "tabla de particiones (GPT)"]),
        "Aucun volume reconnu au début du support : recherche dans les {} premiers..." => Some(["No volume recognised at the start of the media: searching the first {}...", "Ningún volumen reconocido al inicio del soporte: buscando en los primeros {}..."]),
        "recherche sur le support" => Some(["search on the media", "búsqueda en el soporte"]),
        "Emplacement des données inconnu" => Some(["Data location unknown", "Ubicación de los datos desconocida"]),
        "Fichier tronqué à la fin du volume" => Some(["File truncated at the end of the volume", "Archivo truncado al final del volumen"]),
        "Données supposées contiguës, le fichier peut être incomplet" => Some(["Data assumed contiguous, the file may be incomplete", "Datos supuestamente contiguos, el archivo puede estar incompleto"]),
        " Première lettre du nom déduite des fichiers voisins." => Some([" First letter of the name inferred from neighbouring files.", " Primera letra del nombre deducida de los archivos vecinos."]),
        "Table d'allocation abîmée : seul le début du dossier racine a pu être lu." => Some(["Allocation table damaged: only the start of the root folder could be read.", "Tabla de asignación dañada: solo se ha podido leer el inicio de la carpeta raíz."]),
        "Dernier dossier" => Some(["Last folder", "Última carpeta"]),
        "Dossier personnel" => Some(["Home folder", "Carpeta personal"]),
        "Bureau" => Some(["Desktop", "Escritorio"]),
        "Téléchargements" => Some(["Downloads", "Descargas"]),
        "Vidéos" => Some(["Videos", "Vídeos"]),
        "Documents" => Some(["Documents", "Documentos"]),
        "Impossible d'ouvrir ce dossier : " => Some(["Cannot open this folder: ", "No se puede abrir esta carpeta: "]),
        "Un traitement est déjà en cours." => Some(["A job is already running.", "Ya hay un proceso en curso."]),
        "Ajoutez au moins un fichier de référence sain, filmé avec la même caméra et le même mode." => Some(["Add at least one healthy reference file, shot with the same camera and the same mode.", "Añade al menos un archivo de referencia sano, grabado con la misma cámara y el mismo modo."]),
        "Aucun fichier à réparer (liste vide, fichiers introuvables ou tous ignorés)." => Some(["No files to repair (empty list, files not found or all skipped).", "Ningún archivo que reparar (lista vacía, archivos no encontrados o todos ignorados)."]),
        "Choisissez un dossier de sortie." => Some(["Choose an output folder.", "Elige una carpeta de salida."]),
        "untrunc.exe introuvable. Indiquez son emplacement dans Outils." => Some(["untrunc.exe not found. Set its location under Tools.", "No se encuentra untrunc.exe. Indica su ubicación en Herramientas."]),
        "ffmpeg / ffprobe introuvables. Indiquez le dossier qui contient ffprobe.exe dans Outils." => Some(["ffmpeg / ffprobe not found. Set the folder containing ffprobe.exe under Tools.", "No se encuentran ffmpeg / ffprobe. Indica la carpeta que contiene ffprobe.exe en Herramientas."]),
        "Référence introuvable : " => Some(["Reference not found: ", "Referencia no encontrada: "]),
        "Le dossier de sortie doit être différent de celui des fichiers à réparer." => Some(["The output folder must differ from the folder of the files to repair.", "La carpeta de salida debe ser distinta de la de los archivos que se van a reparar."]),
        "{} fichier(s), {} référence(s), sortie : {}" => Some(["{} file(s), {} reference(s), output: {}", "{} archivo(s), {} referencia(s), salida: {}"]),
        "Vérification complète activée." => Some(["Full check enabled.", "Verificación completa activada."]),
        "Traitement arrêté." => Some(["Job stopped.", "Proceso detenido."]),
        "Arrêt demandé, fin de l'opération en cours..." => Some(["Stop requested, finishing the current operation...", "Detención solicitada, terminando la operación en curso..."]),
        "Chemin manquant" => Some(["Missing path", "Falta la ruta"]),
        "Impossible de démarrer le serveur local : " => Some(["Cannot start the local server: ", "No se puede iniciar el servidor local: "]),
        "impossible de verrouiller la carte : fermez les fenêtres et programmes qui l'utilisent, puis réessayez" => Some(["cannot lock the card: close the windows and programs using it, then try again", "no se puede bloquear la tarjeta: cierra las ventanas y programas que la usan y vuelve a intentarlo"]),
        "écriture incomplète" => Some(["incomplete write", "escritura incompleta"]),
        _ => None,
    };
    match t {
        Some(t) => t[(i - 1) as usize].to_string(),
        None => fr.to_string(),
    }
}

/// Remplace les marqueurs {} dans l'ordre (équivalent de Sprintf pour les messages traduits).
pub fn f(fr: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut s = l(fr);
    for a in args {
        if let Some(p) = s.find("{}") {
            s.replace_range(p..p + 2, &a.to_string());
        }
    }
    s
}

