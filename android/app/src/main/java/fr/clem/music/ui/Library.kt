package fr.clem.music.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.DownloadDone
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.painter.ColorPainter
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import coil.compose.rememberAsyncImagePainter
import fr.clem.music.Album
import fr.clem.music.Artist
import fr.clem.music.Link
import fr.clem.music.I18n
import fr.clem.music.MainViewModel
import fr.clem.music.Track
import fr.clem.music.fmtTime
import fr.clem.music.tr

// --- images --------------------------------------------------------------------------------

@Composable
fun AlbumCover(vm: MainViewModel, album: Album?, modifier: Modifier = Modifier, size: Int = 400, corner: Int = 8) {
    val done by vm.downloads.done.collectAsState()
    val link by vm.repo.link.collectAsState()
    val model = remember(album?.key, done, link) {
        album?.let { vm.downloads.coverFile(it) ?: vm.repo.coverUrl(it.first.path, size) }
    }
    val placeholder = ColorPainter(MaterialTheme.colorScheme.surfaceVariant)
    AsyncImage(
        model = model,
        contentDescription = null,
        contentScale = ContentScale.Crop,
        placeholder = placeholder,
        error = placeholder,
        fallback = placeholder,
        modifier = modifier.clip(RoundedCornerShape(corner.dp)),
    )
}

@Composable
fun TrackCover(vm: MainViewModel, path: String?, modifier: Modifier = Modifier, size: Int = 200, corner: Int = 6) {
    val index by vm.repo.index.collectAsState()
    AlbumCover(vm, path?.let { index?.albumOf(it) }, modifier, size, corner)
}

/** Photo de l'artiste ; à défaut, la pochette d'un de ses albums. */
@Composable
fun ArtistImage(vm: MainViewModel, artist: Artist, modifier: Modifier = Modifier, size: Int = 200, corner: Int = 8) {
    val link by vm.repo.link.collectAsState()
    val first = artist.albums.first()
    val cover = remember(artist.key, link) { vm.downloads.coverFile(first) ?: vm.repo.coverUrl(first.first.path, size) }
    val photo = remember(artist.key, link) { vm.repo.artistUrl(artist.name, first.first.path, size) }
    val fallback = rememberAsyncImagePainter(cover)
    AsyncImage(
        model = photo ?: cover,
        contentDescription = null,
        contentScale = ContentScale.Crop,
        placeholder = fallback,
        error = fallback,
        fallback = fallback,
        modifier = modifier.clip(RoundedCornerShape(corner.dp)),
    )
}

// --- listes --------------------------------------------------------------------------------

@Composable
fun ArtistList(vm: MainViewModel, artists: List<Artist>, open: (Artist) -> Unit, header: @Composable () -> Unit = {}) {
    LazyColumn(contentPadding = PaddingValues(bottom = 16.dp), modifier = Modifier.fillMaxSize()) {
        item { header() }
        items(artists, key = { it.key }) { a ->
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth().clickable { open(a) }.padding(horizontal = 16.dp, vertical = 6.dp),
            ) {
                ArtistImage(vm, a, Modifier.size(48.dp), size = 160)
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(a.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        plural(a.albums.size, "album", "albums") + " · " + plural(a.trackCount, "titre", "titres"),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

@Composable
fun AlbumGrid(vm: MainViewModel, albums: List<Album>, open: (Album) -> Unit, header: @Composable () -> Unit = {}) {
    val progress by vm.downloads.progress.collectAsState()
    val done by vm.downloads.done.collectAsState()
    LazyVerticalGrid(
        columns = GridCells.Adaptive(150.dp),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
        modifier = Modifier.fillMaxSize(),
    ) {
        item(span = { GridItemSpan(maxLineSpan) }) { header() }
        items(albums, key = { it.key }) { a ->
            Column(Modifier.clickable { open(a) }) {
                Box {
                    AlbumCover(vm, a, Modifier.fillMaxWidth().aspectRatio(1f))
                    val p = progress[a.key]
                    val offline = a.tracks.all { it.path in done }
                    if (p != null || offline) {
                        Box(
                            Modifier.align(Alignment.BottomEnd).padding(6.dp).size(26.dp).clip(RoundedCornerShape(50))
                                .background(MaterialTheme.colorScheme.surfaceContainer),
                            contentAlignment = Alignment.Center,
                        ) {
                            if (p != null) CircularProgressIndicator(progress = { p }, strokeWidth = 2.dp, modifier = Modifier.size(18.dp))
                            else Icon(Icons.Rounded.DownloadDone, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(16.dp))
                        }
                    }
                }
                Spacer(Modifier.height(6.dp))
                Text(a.title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    listOfNotNull(a.artist, a.year?.toString()).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
@OptIn(ExperimentalFoundationApi::class)
fun TrackRow(
    vm: MainViewModel,
    t: Track,
    number: String,
    current: Boolean,
    showArtist: Boolean = false,
    onLongClick: (() -> Unit)? = null,
    onClick: () -> Unit,
) {
    val done by vm.downloads.done.collectAsState()
    val link by vm.repo.link.collectAsState()
    val coll by vm.collections.collectAsState()
    val local = t.path in done
    // Hors ligne, seules les pistes téléchargées se lisent (la télécommande du PC non plus).
    val enabled = local || link == Link.Online
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth()
            .combinedClickable(enabled = enabled, onClick = onClick, onLongClick = onLongClick)
            .padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
    ) {
        Box(Modifier.width(32.dp).alpha(if (enabled) 1f else 0.4f)) {
            if (current) Icon(Icons.Rounded.GraphicEq, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
            else Text(number, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Column(Modifier.weight(1f).alpha(if (enabled) 1f else 0.4f)) {
            Text(
                t.displayTitle,
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = if (current) FontWeight.SemiBold else FontWeight.Medium,
                color = if (current) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (showArtist) Text(t.artist, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
        }
        if (local) {
            Icon(Icons.Rounded.DownloadDone, null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(14.dp))
            Spacer(Modifier.width(8.dp))
        }
        Text(fmtTime(t.duration), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        HeartButton(t.path in coll.likedTracks, small = true) { vm.toggleLike(t.path) }
    }
}

// --- écrans --------------------------------------------------------------------------------

@Composable
fun ArtistScreen(vm: MainViewModel, artist: Artist, openAlbum: (Album) -> Unit) {
    AlbumGrid(vm, artist.albums, openAlbum, header = {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(vertical = 12.dp)) {
            ArtistImage(vm, artist, Modifier.size(96.dp), size = 400, corner = 12)
            Spacer(Modifier.width(16.dp))
            Column {
                Text(artist.name, style = MaterialTheme.typography.headlineMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(
                    plural(artist.albums.size, "album", "albums") + " · " + plural(artist.trackCount, "titre", "titres"),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    })
}

@Composable
fun AlbumScreen(vm: MainViewModel, album: Album, openArtist: () -> Unit, toast: (String) -> Unit) {
    val now by vm.now.collectAsState()
    val coll by vm.collections.collectAsState()
    var menuFor by remember { mutableStateOf<Track?>(null) }
    var addAll by remember { mutableStateOf(false) }
    menuFor?.let { t -> TrackMenu(vm, t, null, toast) { menuFor = null } }
    if (addAll) AddToPlaylistDialog(vm, album.tracks.map { it.path }, toast) { addAll = false }
    val progress by vm.downloads.progress.collectAsState()
    val done by vm.downloads.done.collectAsState()
    val link by vm.repo.link.collectAsState()
    val p = progress[album.key]
    val offline = album.tracks.all { it.path in done }
    val multiDisc = album.tracks.mapNotNull { it.disc }.distinct().size > 1

    LazyColumn(contentPadding = PaddingValues(bottom = 16.dp), modifier = Modifier.fillMaxSize()) {
        item {
            Column(Modifier.padding(16.dp)) {
                AlbumCover(vm, album, Modifier.fillMaxWidth().aspectRatio(1f), size = 800, corner = 12)
                Spacer(Modifier.height(16.dp))
                Row(verticalAlignment = Alignment.Top) {
                    Column(Modifier.weight(1f)) {
                        Text(album.title, style = MaterialTheme.typography.headlineMedium)
                        Text(
                            album.artist,
                            style = MaterialTheme.typography.titleMedium,
                            color = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.clickable(onClick = openArtist).padding(vertical = 2.dp),
                        )
                    }
                    HeartButton(album.likeKey in coll.likedAlbums) { vm.toggleLikeAlbum(album) }
                    IconButton(onClick = { addAll = true }) {
                        Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, tr("Ajouter à une playlist"))
                    }
                }
                Text(
                    listOfNotNull(album.year?.toString(), plural(album.tracks.size, "titre", "titres"), fmtTime(album.duration), album.first.quality.ifBlank { null })
                        .joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(14.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
                    Button(
                        onClick = { vm.play(album.tracks, 0) },
                        enabled = offline || link == Link.Online,
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primary),
                    ) {
                        Icon(Icons.Rounded.PlayArrow, null)
                        Spacer(Modifier.width(6.dp))
                        Text(tr("Lire"))
                    }
                    OutlinedButton(
                        onClick = { val s = album.tracks.shuffled(); vm.play(s, 0) },
                        enabled = offline || link == Link.Online,
                    ) { Icon(Icons.Rounded.Shuffle, tr("Aléatoire")) }
                    Spacer(Modifier.weight(1f))
                    when {
                        p != null -> OutlinedButton(onClick = { vm.downloads.cancel(album) }) {
                            CircularProgressIndicator(progress = { p }, strokeWidth = 2.dp, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(8.dp))
                            Text("${(p * 100).toInt()} %")
                            Spacer(Modifier.width(6.dp))
                            Icon(Icons.Rounded.Close, tr("Annuler"), modifier = Modifier.size(16.dp))
                        }
                        offline -> OutlinedButton(onClick = { vm.downloads.remove(album); toast(tr("Album retiré du téléphone")) }) {
                            Icon(Icons.Rounded.Delete, null, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text(tr("Retirer"))
                        }
                        else -> OutlinedButton(onClick = { vm.downloads.download(album) }, enabled = link == Link.Online) {
                            Icon(Icons.Rounded.Download, null, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text(tr("Télécharger"))
                        }
                    }
                }
                if (p == null && !offline) {
                    val mb = album.tracks.sumOf { it.size } / 1_000_000.0
                    Text(
                        tr("Hors ligne : %.0f Mo").format(mb),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 6.dp).align(Alignment.End),
                    )
                }
            }
        }
        var lastDisc: Int? = null
        album.tracks.forEachIndexed { i, t ->
            val disc = t.disc ?: 1
            if (multiDisc && disc != lastDisc) {
                lastDisc = disc
                item(key = "disc-$disc") {
                    Text(
                        (if (I18n.en) "DISC $disc" else "DISQUE $disc"),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp),
                    )
                }
            }
            item(key = t.path) {
                TrackRow(vm, t, (t.track ?: (i + 1)).toString().padStart(2, '0'), now.path == t.path, onLongClick = { menuFor = t }) {
                    vm.play(album.tracks, i)
                }
            }
        }
    }
}

@Composable
fun OfflineScreen(vm: MainViewModel, albums: List<Album>, open: (Album) -> Unit, toast: (String) -> Unit) {
    val done by vm.downloads.done.collectAsState()
    val progress by vm.downloads.progress.collectAsState()
    val list = albums.filter { a -> a.key in progress || a.tracks.any { it.path in done } }
    if (list.isEmpty()) {
        Empty(Icons.Rounded.CloudOff, tr("Rien sur le téléphone pour l'instant"), tr("Ouvre un album et touche « Télécharger » pour l'écouter sans réseau."))
        return
    }
    val used = remember(done) { vm.downloads.usedBytes() }
    AlbumGrid(vm, list, open, header = {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(vertical = 8.dp)) {
            Text(
                plural(list.size, "album", "albums") + " · " + tr("%.1f Go").format(used / 1e9),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f),
            )
        }
    })
}

@Composable
fun SearchResults(vm: MainViewModel, q: String, openArtist: (Artist) -> Unit, openAlbum: (Album) -> Unit, toast: (String) -> Unit) {
    var menuFor by remember { mutableStateOf<Track?>(null) }
    menuFor?.let { t -> TrackMenu(vm, t, null, toast) { menuFor = null } }
    val index by vm.repo.index.collectAsState()
    val now by vm.now.collectAsState()
    val (ar, al, tks) = remember(q, index) { index?.search(q) ?: Triple(emptyList(), emptyList(), emptyList()) }
    if (q.isNotBlank() && ar.isEmpty() && al.isEmpty() && tks.isEmpty()) {
        Empty(Icons.Rounded.MusicNote, tr("Aucun résultat"), if (I18n.en) "Nothing matches “$q”." else "Rien ne correspond à « $q ».")
        return
    }
    LazyColumn(contentPadding = PaddingValues(bottom = 16.dp), modifier = Modifier.fillMaxSize()) {
        if (ar.isNotEmpty()) {
            item { Section(tr("Artistes")) }
            items(ar, key = { "a" + it.key }) { a ->
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier.fillMaxWidth().clickable { openArtist(a) }.padding(horizontal = 16.dp, vertical = 6.dp),
                ) {
                    ArtistImage(vm, a, Modifier.size(44.dp), size = 160)
                    Spacer(Modifier.width(14.dp))
                    Text(a.name, style = MaterialTheme.typography.bodyLarge)
                }
            }
        }
        if (al.isNotEmpty()) {
            item { Section("Albums") }
            items(al, key = { "b" + it.key }) { a ->
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier.fillMaxWidth().clickable { openAlbum(a) }.padding(horizontal = 16.dp, vertical = 6.dp),
                ) {
                    AlbumCover(vm, a, Modifier.size(44.dp), size = 160, corner = 6)
                    Spacer(Modifier.width(14.dp))
                    Column {
                        Text(a.title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(a.artist, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
        }
        if (tks.isNotEmpty()) {
            item { Section(tr("Titres")) }
            items(tks, key = { "t" + it.path }) { t ->
                TrackRow(vm, t, "", now.path == t.path, showArtist = true, onLongClick = { menuFor = t }) {
                    val album = index?.albumOf(t.path)
                    if (album != null) vm.play(album.tracks, album.tracks.indexOf(t)) else vm.play(listOf(t), 0)
                }
            }
        }
    }
}

@Composable
fun Section(title: String) {
    Text(
        title.uppercase(),
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(start = 16.dp, top = 18.dp, bottom = 6.dp),
    )
}

@Composable
fun Empty(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, text: String) {
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(40.dp))
        Spacer(Modifier.height(12.dp))
        Text(title, style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(4.dp))
        Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

fun plural(n: Int, one: String, many: String) =
    if (I18n.en) "%,d %s".format(java.util.Locale.US, n, tr(if (n != 1) many else one))
    else "%,d %s".format(n, if (n > 1) many else one).replace(',', ' ')
