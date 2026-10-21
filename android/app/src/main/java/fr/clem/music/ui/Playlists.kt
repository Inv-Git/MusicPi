package fr.clem.music.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.DownloadDone
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FavoriteBorder
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.KeyboardArrowUp
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import fr.clem.music.Album
import fr.clem.music.Link
import fr.clem.music.I18n
import fr.clem.music.MainViewModel
import fr.clem.music.Track
import fr.clem.music.fmtTime
import fr.clem.music.tr

// --- petits éléments ------------------------------------------------------------------------

/** Le petit cœur : plein et corail quand c'est aimé. */
@Composable
fun HeartButton(liked: Boolean, small: Boolean = false, onClick: () -> Unit) {
    IconButton(onClick = onClick, modifier = if (small) Modifier.size(40.dp) else Modifier) {
        Icon(
            if (liked) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder,
            if (liked) tr("Je n'aime plus") else tr("J'aime"),
            tint = if (liked) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(if (small) 18.dp else 24.dp),
        )
    }
}

@Composable
private fun Tile(icon: ImageVector, accent: Boolean, size: Int = 48) {
    Box(
        contentAlignment = Alignment.Center,
        modifier = Modifier.size(size.dp).clip(RoundedCornerShape(8.dp))
            .background(if (accent) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Icon(icon, null, tint = if (accent) Color.White else MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size((size / 2).dp))
    }
}

/** Fenêtre pour nommer une playlist (création ou renommage). */
@Composable
fun NameDialog(title: String, initial: String, confirm: String, onDismiss: () -> Unit, onConfirm: (String) -> Unit) {
    var name by remember { mutableStateOf(initial) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                singleLine = true,
                label = { Text(tr("Nom")) },
                modifier = Modifier.fillMaxWidth().focusRequester(focus),
            )
        },
        confirmButton = { TextButton(onClick = { onConfirm(name.trim()) }, enabled = name.isNotBlank()) { Text(confirm) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Annuler")) } },
    )
}

/** Choix de la playlist où ajouter des titres, ou création d'une nouvelle. */
@Composable
fun AddToPlaylistDialog(vm: MainViewModel, paths: List<String>, toast: (String) -> Unit, onDismiss: () -> Unit) {
    val coll by vm.collections.collectAsState()
    var naming by remember { mutableStateOf(false) }
    val what = plural(paths.size, "titre", "titres")
    if (naming) {
        NameDialog(tr("Nouvelle playlist"), "", tr("Créer"), onDismiss) { name ->
            vm.createPlaylist(name, paths)
            toast(if (I18n.en) "Playlist “$name” created with $what" else "Playlist « $name » créée avec $what")
            onDismiss()
        }
        return
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (I18n.en) "Add $what to…" else "Ajouter $what à…") },
        text = {
            Column(Modifier.heightIn(max = 420.dp).verticalScroll(rememberScrollState())) {
                DialogRow(Icons.Rounded.Add, tr("Nouvelle playlist"), accent = true) { naming = true }
                coll.playlists.forEach { p ->
                    DialogRow(Icons.AutoMirrored.Rounded.QueueMusic, p.name, sub = plural(p.tracks.size, "titre", "titres")) {
                        vm.addToPlaylist(p.id, paths)
                        toast(if (I18n.en) "Added to “${p.name}”" else "Ajouté à « ${p.name} »")
                        onDismiss()
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Annuler")) } },
    )
}

@Composable
private fun DialogRow(icon: ImageVector, label: String, sub: String? = null, accent: Boolean = false, onClick: () -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).clickable(onClick = onClick).padding(vertical = 8.dp, horizontal = 4.dp),
    ) {
        Tile(icon, accent, size = 40)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(label, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (sub != null) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** Appui long sur un titre : cœur, ajout à une playlist, retrait de la playlist affichée. */
@Composable
fun TrackMenu(vm: MainViewModel, t: Track, playlistId: String?, toast: (String) -> Unit, onDismiss: () -> Unit) {
    val coll by vm.collections.collectAsState()
    var adding by remember { mutableStateOf(false) }
    if (adding) {
        AddToPlaylistDialog(vm, listOf(t.path), toast, onDismiss)
        return
    }
    val liked = t.path in coll.likedTracks
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(t.displayTitle, maxLines = 2, overflow = TextOverflow.Ellipsis) },
        text = {
            Column {
                DialogRow(if (liked) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder, if (liked) tr("Je n'aime plus") else tr("J'aime"), accent = liked) {
                    vm.toggleLike(t.path)
                    onDismiss()
                }
                DialogRow(Icons.AutoMirrored.Rounded.PlaylistAdd, tr("Ajouter à une playlist…")) { adding = true }
                if (playlistId != null && playlistId != "liked") {
                    DialogRow(Icons.Rounded.Delete, tr("Retirer de la playlist")) {
                        vm.removeFromPlaylist(playlistId, listOf(t.path))
                        onDismiss()
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text(tr("Fermer")) } },
    )
}

// --- écrans -------------------------------------------------------------------------------

@Composable
fun PlaylistsScreen(vm: MainViewModel, openPlaylist: (String) -> Unit, openLikedAlbums: () -> Unit) {
    val coll by vm.collections.collectAsState()
    val index by vm.repo.index.collectAsState()
    var naming by remember { mutableStateOf(false) }
    if (naming) {
        NameDialog(tr("Nouvelle playlist"), "", tr("Créer"), { naming = false }) { name ->
            naming = false
            openPlaylist(vm.createPlaylist(name))
        }
    }
    val likedAlbums = index?.albums?.count { it.likeKey in coll.likedAlbums } ?: 0
    LazyColumn(contentPadding = PaddingValues(vertical = 8.dp), modifier = Modifier.fillMaxSize()) {
        item {
            ListRow(Icons.Rounded.Favorite, true, tr("Titres aimés"), plural(coll.likedTracks.size, "titre", "titres")) { openPlaylist("liked") }
            ListRow(Icons.Rounded.Favorite, true, tr("Albums aimés"), plural(likedAlbums, "album", "albums"), onClick = openLikedAlbums)
            HorizontalDivider(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), color = MaterialTheme.colorScheme.outlineVariant)
        }
        items(coll.playlists, key = { it.id }) { p ->
            ListRow(Icons.AutoMirrored.Rounded.QueueMusic, false, p.name, plural(p.tracks.size, "titre", "titres")) { openPlaylist(p.id) }
        }
        item { ListRow(Icons.Rounded.Add, false, tr("Nouvelle playlist"), null) { naming = true } }
    }
}

@Composable
private fun ListRow(icon: ImageVector, accent: Boolean, title: String, sub: String?, onClick: () -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 6.dp),
    ) {
        Tile(icon, accent)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (sub != null) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
fun LikedAlbumsScreen(vm: MainViewModel, openAlbum: (Album) -> Unit) {
    val coll by vm.collections.collectAsState()
    val index by vm.repo.index.collectAsState()
    val list = index?.albums?.filter { it.likeKey in coll.likedAlbums }.orEmpty()
    if (list.isEmpty()) {
        Empty(Icons.Rounded.FavoriteBorder, tr("Aucun album aimé"), tr("Touche le cœur sur la page d'un album pour le retrouver ici."))
        return
    }
    AlbumGrid(vm, list, openAlbum, header = {
        Text(plural(list.size, "album", "albums"), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(vertical = 8.dp))
    })
}

/** Une playlist, ou les titres aimés (`id` = « liked »). */
@Composable
fun PlaylistScreen(vm: MainViewModel, id: String, toast: (String) -> Unit, close: () -> Unit) {
    val coll by vm.collections.collectAsState()
    val index by vm.repo.index.collectAsState()
    val now by vm.now.collectAsState()
    val link by vm.repo.link.collectAsState()
    val done by vm.downloads.done.collectAsState()
    val progress by vm.downloads.progress.collectAsState()
    val playlist = coll.playlists.firstOrNull { it.id == id }
    val liked = id == "liked"
    if (!liked && playlist == null) {
        Empty(Icons.AutoMirrored.Rounded.QueueMusic, tr("Playlist introuvable"), tr("Elle a peut-être été supprimée depuis le PC."))
        return
    }
    val name = playlist?.name ?: tr("Titres aimés")
    val paths = if (liked) coll.likedTracks else playlist!!.tracks
    val tracks = paths.mapNotNull { index?.byPath?.get(it) }
    val playable = tracks.filter { it.path in done || link == Link.Online }
    val p = progress["pl:$id"]
    val offline = tracks.isNotEmpty() && tracks.all { it.path in done }

    var menuFor by remember { mutableStateOf<Pair<Int, Track>?>(null) }
    var renaming by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }
    var reorder by remember { mutableStateOf(false) }
    menuFor?.let { (_, t) -> TrackMenu(vm, t, id, toast) { menuFor = null } }
    if (renaming && playlist != null) {
        NameDialog(tr("Renommer la playlist"), playlist.name, tr("Renommer"), { renaming = false }) { n ->
            vm.renamePlaylist(id, n)
            renaming = false
        }
    }
    if (confirmDelete && playlist != null) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text(if (I18n.en) "Delete “${playlist.name}”?" else "Supprimer « ${playlist.name} » ?") },
            text = { Text(tr("La playlist disparaît aussi sur le PC. Les morceaux, eux, ne sont pas touchés.")) },
            confirmButton = {
                TextButton(onClick = {
                    confirmDelete = false
                    vm.deletePlaylist(id)
                    toast(tr("Playlist supprimée"))
                    close()
                }) { Text(tr("Supprimer"), color = MaterialTheme.colorScheme.primary) }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text(tr("Annuler")) } },
        )
    }

    LazyColumn(contentPadding = PaddingValues(bottom = 16.dp), modifier = Modifier.fillMaxSize()) {
        item {
            Column(Modifier.padding(16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Tile(if (liked) Icons.Rounded.Favorite else Icons.AutoMirrored.Rounded.QueueMusic, liked, size = 72)
                    Spacer(Modifier.width(16.dp))
                    Column(Modifier.weight(1f)) {
                        Text(name, style = MaterialTheme.typography.headlineMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        Text(
                            plural(tracks.size, "titre", "titres") + " · " + fmtTime(tracks.sumOf { it.duration }),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                Spacer(Modifier.height(14.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Button(
                        onClick = { vm.play(playable, 0) },
                        enabled = playable.isNotEmpty(),
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primary),
                    ) {
                        Icon(Icons.Rounded.PlayArrow, null)
                        Spacer(Modifier.width(6.dp))
                        Text(tr("Lire"))
                    }
                    IconButton(onClick = { vm.play(playable.shuffled(), 0) }, enabled = playable.size > 1) { Icon(Icons.Rounded.Shuffle, tr("Aléatoire")) }
                    when {
                        p != null -> IconButton(onClick = { vm.downloads.cancel("pl:$id") }) {
                            CircularProgressIndicator(progress = { p }, strokeWidth = 2.dp, modifier = Modifier.size(22.dp))
                        }
                        offline -> IconButton(onClick = {
                            vm.downloads.removeTracks(tracks)
                            toast(tr("Titres retirés du téléphone"))
                        }) { Icon(Icons.Rounded.DownloadDone, tr("Retirer du téléphone"), tint = MaterialTheme.colorScheme.primary) }
                        else -> IconButton(onClick = { vm.downloadPlaylist(id, name) }, enabled = link == Link.Online && tracks.isNotEmpty()) {
                            Icon(Icons.Rounded.Download, tr("Télécharger"))
                        }
                    }
                    Spacer(Modifier.weight(1f))
                    if (playlist != null) {
                        IconButton(onClick = { reorder = !reorder }, enabled = tracks.size > 1) {
                            Icon(
                                if (reorder) Icons.Rounded.Close else Icons.Rounded.KeyboardArrowUp,
                                if (reorder) tr("Terminer") else tr("Réordonner"),
                                tint = if (reorder) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        IconButton(onClick = { renaming = true }) { Icon(Icons.Rounded.Edit, tr("Renommer")) }
                        IconButton(onClick = { confirmDelete = true }) { Icon(Icons.Rounded.Delete, tr("Supprimer")) }
                    }
                }
                if (reorder) {
                    Text(tr("Touche les flèches pour déplacer les titres."), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
                }
            }
        }
        if (tracks.isEmpty()) {
            item {
                Text(
                    if (liked) tr("Touche le cœur à côté d'un titre pour le retrouver ici.")
                    else tr("Playlist vide. Appui long sur un titre, ou le bouton « Ajouter à une playlist » d'un album."),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 24.dp),
                )
            }
        }
        // Positions réelles dans la playlist (des titres absents de la bibliothèque peuvent être sautés).
        val rows = paths.mapIndexedNotNull { pos, path -> index?.byPath?.get(path)?.let { pos to it } }
        itemsIndexed(rows, key = { _, r -> "${r.first}|${r.second.path}" }) { i, (pos, t) ->
            if (reorder && playlist != null) {
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp)) {
                    Text(t.displayTitle, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                    IconButton(onClick = { vm.movePlaylistTrack(id, pos, pos - 1) }, enabled = i > 0) { Icon(Icons.Rounded.KeyboardArrowUp, tr("Monter")) }
                    IconButton(onClick = { vm.movePlaylistTrack(id, pos, pos + 1) }, enabled = i < rows.size - 1) { Icon(Icons.Rounded.KeyboardArrowDown, tr("Descendre")) }
                }
            } else {
                TrackRow(vm, t, (i + 1).toString(), now.path == t.path, showArtist = true, onLongClick = { menuFor = pos to t }) {
                    val k = playable.indexOf(t)
                    if (k >= 0) vm.play(playable, k)
                }
            }
        }
        if (!liked && tracks.size < paths.size) {
            item {
                Text(
                    plural(paths.size - tracks.size, "titre introuvable", "titres introuvables") + tr(" dans la bibliothèque (déplacé ou supprimé)."),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(16.dp),
                )
            }
        }
    }
}
