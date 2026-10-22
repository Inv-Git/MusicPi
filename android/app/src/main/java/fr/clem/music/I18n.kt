package fr.clem.music

import java.util.Locale

/**
 * Traduction de l'app (français → anglais). Le français reste le texte de référence dans le code ;
 * si le téléphone n'est pas réglé en français, tr() renvoie la version anglaise.
 */
object I18n {
    val en: Boolean get() = Locale.getDefault().language != "fr"

    val EN = mapOf(
        // Navigation
        "Artistes" to "Artists",
        "Hors ligne" to "Offline",
        "Réglages" to "Settings",
        "Albums aimés" to "Liked albums",
        "Titres aimés" to "Liked tracks",
        "Titres" to "Tracks",
        "Retour" to "Back",
        "Rechercher" to "Search",
        "Fermer la recherche" to "Close search",
        "Artiste, album, titre…" to "Artist, album, track…",
        "Aucun résultat" to "No results",
        "Artiste inconnu" to "Unknown artist",
        "Album inconnu" to "Unknown album",
        // Connexion
        "Chargement de la bibliothèque…" to "Loading library…",
        "En attente du PC" to "Waiting for the PC",
        "La bibliothèque s'affiche dès que le PC répond (MusicPi ouvert, partage activé)." to "The library shows up as soon as the PC responds (MusicPi open, sharing on).",
        "Relier au PC" to "Link to the PC",
        "Relier" to "Link",
        "Connexion…" to "Connecting…",
        "Sur le PC, ouvre MusicPi › Paramètres › Téléphone, active le partage, puis recopie ici l'adresse « à la maison » et le code à 6 chiffres. Le téléphone doit être sur le même Wi-Fi pour cette première fois." to
            "On the PC, open MusicPi › Settings › Phone, turn sharing on, then enter the “at home” address and the 6-digit code here. The phone must be on the same Wi-Fi this first time.",
        "Adresse du PC" to "PC address",
        "Code" to "Code",
        "Adresses connues : " to "Known addresses: ",
        "Recherche du PC…" to "Looking for the PC…",
        "Injoignable : seuls les albums téléchargés sont disponibles." to "Unreachable: only downloaded albums are available.",
        "Hors de chez toi, installe Tailscale sur le PC et sur le téléphone (même compte) : l'app retrouvera le PC toute seule." to
            "Away from home, install Tailscale on the PC and on the phone (same account): the app will find the PC on its own.",
        "Reconnecter" to "Reconnect",
        "Confirmer" to "Confirm",
        "Délier ce téléphone" to "Unlink this phone",
        "PC injoignable" to "PC unreachable",
        "PC injoignable. Vérifie l'adresse, que le partage est activé dans Paramètres › Téléphone, et que le pare-feu Windows l'autorise." to
            "PC unreachable. Check the address, that sharing is on in Settings › Phone, and that the Windows firewall allows it.",
        "PC injoignable : vérifie qu'il est allumé et que MusicPi est ouvert." to "PC unreachable: check that it's on and that MusicPi is open.",
        "Réponse inattendue." to "Unexpected response.",
        // Réglages
        "Qualité" to "Quality",
        "Économiser les données mobiles" to "Save mobile data",
        "En 4G, le PC envoie une version allégée (Opus 160 kbit/s, environ 8 fois plus légère). En Wi-Fi et pour les téléchargements : toujours le fichier d'origine." to
            "On mobile data, the PC sends a lighter version (Opus 160 kbit/s, about 8 times smaller). On Wi-Fi and for downloads: always the original file.",
        "%.2f Go utilisés sur le téléphone" to "%.2f GB used on the phone",
        "Téléchargements supprimés" to "Downloads deleted",
        "Confirmer la suppression" to "Confirm deletion",
        "Supprimer tous les téléchargements" to "Delete all downloads",
        "À propos" to "About",
        "MusicPi 1.1.0 · Les fichiers téléchargés sont les originaux (FLAC), ils restent dans l'espace privé de l'app." to
            "MusicPi 1.1.0 · Downloaded files are the originals (FLAC); they stay in the app's private storage.",
        // Bibliothèque, albums
        "Ajouter à une playlist" to "Add to playlist",
        "Ajouter à une playlist…" to "Add to playlist…",
        "Lire" to "Play",
        "Aléatoire" to "Shuffle",
        "Album retiré du téléphone" to "Album removed from the phone",
        "Retirer" to "Remove",
        "Télécharger" to "Download",
        "Rien sur le téléphone pour l'instant" to "Nothing on the phone yet",
        "Ouvre un album et touche « Télécharger » pour l'écouter sans réseau." to "Open an album and tap “Download” to listen without a connection.",
        "%.1f Go" to "%.1f GB",
        "Hors ligne : %.0f Mo" to "Offline: %.0f MB",
        "Téléphone" to "Phone",
        // Lecteur
        "Rien en lecture sur le PC" to "Nothing playing on the PC",
        "Lecture" to "Play",
        "Lecture / pause" to "Play / pause",
        "Précédent" to "Previous",
        "Suivant" to "Next",
        "Masquer le mode travail" to "Hide practice mode",
        "Mode travail : vitesse et boucle" to "Practice mode: speed and loop",
        "VITESSE" to "SPEED",
        "BOUCLE" to "LOOP",
        "Le module de vitesse (bass_fx.dll) manque sur le PC." to "The speed module (bass_fx.dll) is missing on the PC.",
        "La hauteur des notes ne change pas." to "The pitch doesn't change.",
        "Début" to "Start",
        "Fin" to "End",
        "Effacer la boucle" to "Clear loop",
        "Touche « Début » au début du passage, puis « Fin » : il tourne en boucle jusqu'à ce que tu l'effaces." to
            "Tap “Start” at the beginning of the passage, then “End”: it loops until you clear it.",
        "La fin de la boucle doit être après son début." to "The loop end must be after its start.",
        // Playlists
        "J'aime" to "Like",
        "Je n'aime plus" to "Unlike",
        "Nom" to "Name",
        "Annuler" to "Cancel",
        "Créer" to "Create",
        "Fermer" to "Close",
        "Nouvelle playlist" to "New playlist",
        "Retirer de la playlist" to "Remove from playlist",
        "Aucun album aimé" to "No liked albums",
        "Touche le cœur sur la page d'un album pour le retrouver ici." to "Tap the heart on an album's page to find it here.",
        "Touche le cœur à côté d'un titre pour le retrouver ici." to "Tap the heart next to a track to find it here.",
        "Playlist introuvable" to "Playlist not found",
        "Elle a peut-être été supprimée depuis le PC." to "It may have been deleted from the PC.",
        "Renommer la playlist" to "Rename playlist",
        "Renommer" to "Rename",
        "Supprimer" to "Delete",
        "La playlist disparaît aussi sur le PC. Les morceaux, eux, ne sont pas touchés." to "The playlist is also removed on the PC. The tracks themselves aren't touched.",
        "Playlist supprimée" to "Playlist deleted",
        "Titres retirés du téléphone" to "Tracks removed from the phone",
        "Retirer du téléphone" to "Remove from the phone",
        "Terminer" to "Done",
        "Réordonner" to "Reorder",
        "Touche les flèches pour déplacer les titres." to "Tap the arrows to move tracks.",
        "Playlist vide. Appui long sur un titre, ou le bouton « Ajouter à une playlist » d'un album." to "Empty playlist. Long-press a track, or use an album's “Add to playlist” button.",
        "Monter" to "Move up",
        "Descendre" to "Move down",
        " dans la bibliothèque (déplacé ou supprimé)." to " in the library (moved or deleted).",
        // Pluriels
        "titre" to "track", "titres" to "tracks",
        "album" to "album", "albums" to "albums",
        "titre introuvable" to "track not found", "titres introuvables" to "tracks not found",
        // Paroles
        "Paroles" to "Lyrics",
        "Recherche des paroles…" to "Looking for lyrics…",
        "Morceau instrumental." to "Instrumental track.",
        "Pas de paroles pour ce morceau." to "No lyrics for this track.",
        // Erreurs (dont celles renvoyées par le PC)
        "Trop d'essais : génère un nouveau code sur le PC." to "Too many attempts: generate a new code on the PC.",
        "Appareil non appairé" to "Device not paired",
        "Code incorrect." to "Wrong code.",
        "réseau" to "network",
        "écriture impossible" to "can't write",
    )
}

/** Texte de l'interface dans la langue du téléphone. */
fun tr(fr: String): String = if (I18n.en) I18n.EN[fr] ?: fr else fr
