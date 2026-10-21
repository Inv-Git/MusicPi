package fr.clem.music

import android.Manifest
import android.os.Build
import android.os.Bundle
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.DownloadDone
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.zIndex
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import fr.clem.music.ui.AlbumGrid
import fr.clem.music.ui.AlbumScreen
import fr.clem.music.ui.ArtistList
import fr.clem.music.ui.ArtistScreen
import fr.clem.music.ui.Empty
import fr.clem.music.ui.FullPlayer
import fr.clem.music.ui.LikedAlbumsScreen
import fr.clem.music.ui.PlaylistScreen
import fr.clem.music.ui.PlaylistsScreen
import fr.clem.music.ui.MiniPlayer
import fr.clem.music.ui.MusicTheme
import fr.clem.music.ui.OfflineScreen
import fr.clem.music.ui.SearchResults
import fr.clem.music.ui.Section
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val askNotifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}
    private val vm: MainViewModel by viewModels()

    // En mode télécommande, les touches volume du téléphone règlent le volume du PC.
    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        val up = when (keyCode) {
            KeyEvent.KEYCODE_VOLUME_UP -> true
            KeyEvent.KEYCODE_VOLUME_DOWN -> false
            else -> return super.onKeyDown(keyCode, event)
        }
        return vm.volumeKey(up) || super.onKeyDown(keyCode, event)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        // Pour les contrôles de lecture dans la barre de notifications.
        if (Build.VERSION.SDK_INT >= 33) askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        setContent { MusicTheme { App(vm) } }
    }
}

/** Écrans empilés (le bouton retour remonte d'un cran). */
sealed interface Screen {
    data object Home : Screen
    data class ArtistPage(val key: String) : Screen
    data class AlbumPage(val key: String) : Screen
    data object Settings : Screen
    data class PlaylistPage(val id: String) : Screen
    data object LikedAlbums : Screen
}

enum class Tab(val label: String) { Artists(tr("Artistes")), Albums("Albums"), Playlists("Playlists"), Offline(tr("Hors ligne")) }

@Composable
fun App(vm: MainViewModel) {
    val paired by vm.repo.paired.collectAsState()
    if (!paired) {
        PairScreen(vm)
        return
    }
    val index by vm.repo.index.collectAsState()
    val link by vm.repo.link.collectAsState()
    val dlError by vm.downloads.error.collectAsState()
    val stack = remember { mutableStateListOf<Screen>(Screen.Home) }
    var tab by rememberSaveable { mutableStateOf(Tab.Artists) }
    var query by rememberSaveable { mutableStateOf("") }
    var searching by rememberSaveable { mutableStateOf(false) }
    var playerOpen by rememberSaveable { mutableStateOf(false) }
    val snack = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val toast: (String) -> Unit = { m -> scope.launch { snack.showSnackbar(m) } }
    val top = stack.last()

    LaunchedEffect(dlError) { dlError?.let { snack.showSnackbar(it) } }

    BackHandler(enabled = playerOpen || stack.size > 1 || searching) {
        when {
            playerOpen -> playerOpen = false
            stack.size > 1 -> stack.removeAt(stack.lastIndex)
            else -> { searching = false; query = "" }
        }
    }
    val openArtist: (String) -> Unit = { k -> stack.add(Screen.ArtistPage(k)); playerOpen = false }
    val openAlbum: (String) -> Unit = { k -> stack.add(Screen.AlbumPage(k)); playerOpen = false }

    Box(Modifier.fillMaxSize()) {
        Scaffold(
            snackbarHost = { SnackbarHost(snack) },
            topBar = {
                TopBar(
                    title = when (top) {
                        Screen.Home -> "MusicPi"
                        Screen.Settings -> tr("Réglages")
                        Screen.LikedAlbums -> tr("Albums aimés")
                        else -> ""
                    },
                    back = if (stack.size > 1) ({ stack.removeAt(stack.lastIndex) }) else null,
                    link = link,
                    searching = searching && top == Screen.Home,
                    query = query,
                    onQuery = { query = it },
                    onSearch = { searching = true },
                    onCloseSearch = { searching = false; query = "" },
                    onSettings = { if (top != Screen.Settings) stack.add(Screen.Settings) },
                )
            },
            bottomBar = {
                Column {
                    MiniPlayer(vm) { playerOpen = true }
                    if (top == Screen.Home && !searching) {
                        NavigationBar(containerColor = MaterialTheme.colorScheme.surfaceContainer) {
                            Tab.entries.forEach { t ->
                                NavigationBarItem(
                                    selected = tab == t,
                                    onClick = { tab = t },
                                    icon = {
                                        Icon(
                                            when (t) {
                                                Tab.Artists -> Icons.Rounded.Person
                                                Tab.Albums -> Icons.Rounded.Album
                                                Tab.Playlists -> Icons.AutoMirrored.Rounded.QueueMusic
                                                Tab.Offline -> Icons.Rounded.DownloadDone
                                            },
                                            null,
                                        )
                                    },
                                    label = { Text(t.label) },
                                    colors = NavigationBarItemDefaults.colors(
                                        selectedIconColor = MaterialTheme.colorScheme.primary,
                                        selectedTextColor = MaterialTheme.colorScheme.primary,
                                        indicatorColor = MaterialTheme.colorScheme.primaryContainer,
                                    ),
                                )
                            }
                        }
                    } else {
                        Spacer(Modifier.navigationBarsPadding())
                    }
                }
            },
            containerColor = MaterialTheme.colorScheme.background,
        ) { pad ->
            Box(Modifier.padding(pad).fillMaxSize()) {
                val idx = index
                when {
                    idx == null -> Empty(
                        Icons.Rounded.CloudOff,
                        if (link == Link.Online) tr("Chargement de la bibliothèque…") else tr("En attente du PC"),
                        tr("La bibliothèque s'affiche dès que le PC répond (MusicPi ouvert, partage activé)."),
                    )
                    top is Screen.ArtistPage -> idx.artist(top.key)?.let { a -> ArtistScreen(vm, a) { openAlbum(it.key) } }
                    top is Screen.AlbumPage -> idx.album(top.key)?.let { a -> AlbumScreen(vm, a, { openArtist(a.artistKey) }, toast) }
                    top == Screen.Settings -> SettingsScreen(vm, toast)
                    top is Screen.PlaylistPage -> PlaylistScreen(vm, top.id, toast) { if (stack.size > 1) stack.removeAt(stack.lastIndex) }
                    top == Screen.LikedAlbums -> LikedAlbumsScreen(vm) { openAlbum(it.key) }
                    searching -> SearchResults(vm, query, { openArtist(it.key) }, { openAlbum(it.key) }, toast)
                    tab == Tab.Artists -> ArtistList(vm, idx.artists, { openArtist(it.key) })
                    tab == Tab.Albums -> AlbumGrid(vm, idx.albums, { openAlbum(it.key) })
                    tab == Tab.Playlists -> PlaylistsScreen(vm, { id -> stack.add(Screen.PlaylistPage(id)) }, { stack.add(Screen.LikedAlbums) })
                    else -> OfflineScreen(vm, idx.albums, { openAlbum(it.key) }, toast)
                }
            }
        }
        AnimatedVisibility(
            visible = playerOpen,
            enter = slideInVertically { it },
            exit = slideOutVertically { it },
        ) {
            FullPlayer(
                vm,
                close = { playerOpen = false },
                openAlbum = { path -> index?.albumOf(path)?.let { openAlbum(it.key) } },
                toast = toast,
            )
        }
        VolumeFlash(vm, Modifier.align(Alignment.TopCenter))
    }
}

/** Pastille « Volume du PC » affichée un instant après un appui sur les touches volume. */
@Composable
private fun VolumeFlash(vm: MainViewModel, modifier: Modifier) {
    val flash by vm.volumeFlash.collectAsState()
    var visible by remember { mutableStateOf(false) }
    LaunchedEffect(flash) {
        if (flash == null) return@LaunchedEffect
        visible = true
        kotlinx.coroutines.delay(1500)
        visible = false
    }
    AnimatedVisibility(visible = visible, enter = fadeIn(), exit = fadeOut(), modifier = modifier.zIndex(10f)) {
        val v = flash?.first ?: 0.0
        Surface(
            color = MaterialTheme.colorScheme.inverseSurface,
            contentColor = MaterialTheme.colorScheme.inverseOnSurface,
            shape = RoundedCornerShape(50),
            modifier = Modifier.statusBarsPadding().padding(top = 12.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(horizontal = 16.dp, vertical = 10.dp)) {
                Icon(Icons.Rounded.Computer, null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(10.dp))
                LinearProgressIndicator(
                    progress = { v.toFloat() },
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.width(120.dp),
                )
                Spacer(Modifier.width(10.dp))
                Text("${Math.round(v * 100)} %", style = MaterialTheme.typography.labelLarge)
            }
        }
    }
}

@Composable
private fun TopBar(
    title: String,
    back: (() -> Unit)?,
    link: Link,
    searching: Boolean,
    query: String,
    onQuery: (String) -> Unit,
    onSearch: () -> Unit,
    onCloseSearch: () -> Unit,
    onSettings: () -> Unit,
) {
    Surface(color = MaterialTheme.colorScheme.background) {
        Column(Modifier.statusBarsPadding()) {
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 4.dp)) {
                if (searching) {
                    TextField(
                        value = query,
                        onValueChange = onQuery,
                        placeholder = { Text(tr("Artiste, album, titre…")) },
                        singleLine = true,
                        leadingIcon = { Icon(Icons.Rounded.Search, null) },
                        trailingIcon = { IconButton(onClick = onCloseSearch) { Icon(Icons.Rounded.Close, tr("Fermer la recherche")) } },
                        shape = RoundedCornerShape(12.dp),
                        colors = TextFieldDefaults.colors(
                            focusedIndicatorColor = Color.Transparent,
                            unfocusedIndicatorColor = Color.Transparent,
                        ),
                        modifier = Modifier.weight(1f).padding(horizontal = 8.dp),
                    )
                } else {
                    if (back != null) IconButton(onClick = back) { Icon(Icons.AutoMirrored.Rounded.ArrowBack, tr("Retour")) }
                    else Spacer(Modifier.width(12.dp))
                    Text(title, style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
                    if (link != Link.Online) {
                        Text(
                            if (link == Link.Connecting) tr("Connexion…") else tr("Hors ligne"),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.padding(end = 4.dp),
                        )
                    }
                    IconButton(onClick = onSearch) { Icon(Icons.Rounded.Search, tr("Rechercher")) }
                    IconButton(onClick = onSettings) { Icon(Icons.Rounded.Settings, tr("Réglages")) }
                }
            }
        }
    }
}

@Composable
fun PairScreen(vm: MainViewModel) {
    var address by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
        Column(
            Modifier.fillMaxSize().statusBarsPadding().imePadding().verticalScroll(rememberScrollState()).padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Spacer(Modifier.height(24.dp))
            Text(tr("Relier au PC"), style = MaterialTheme.typography.headlineMedium)
            Text(
                tr("Sur le PC, ouvre MusicPi › Paramètres › Téléphone, active le partage, puis recopie ici l'adresse « à la maison » et le code à 6 chiffres. Le téléphone doit être sur le même Wi-Fi pour cette première fois."),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = address,
                onValueChange = { address = it.trim() },
                label = { Text(tr("Adresse du PC")) },
                placeholder = { Text("192.168.1.20:47800") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = code,
                onValueChange = { code = it.filter(Char::isDigit).take(6) },
                label = { Text("Code") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword),
                modifier = Modifier.fillMaxWidth(),
            )
            error?.let { Text(it, color = MaterialTheme.colorScheme.primary) }
            Button(
                onClick = {
                    busy = true
                    error = null
                    scope.launch {
                        error = vm.repo.pair(address, code)
                        busy = false
                    }
                },
                enabled = !busy && address.isNotBlank() && code.length == 6,
                colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primary),
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) { Text(if (busy) tr("Connexion…") else tr("Relier")) }
        }
    }
}

@Composable
fun SettingsScreen(vm: MainViewModel, toast: (String) -> Unit) {
    val link by vm.repo.link.collectAsState()
    val done by vm.downloads.done.collectAsState()
    val used = remember(done) { vm.downloads.usedBytes() }
    var confirmWipe by remember { mutableStateOf(false) }
    var confirmUnpair by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
        Section("PC")
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(vm.repo.prefs.pcName.ifBlank { "PC" }, style = MaterialTheme.typography.titleMedium)
            Text(
                when (link) {
                    Link.Online -> (if (I18n.en) "Connected" else "Connecté") + " (${vm.repo.base?.host ?: ""})"
                    Link.Connecting -> tr("Recherche du PC…")
                    Link.Offline -> tr("Injoignable : seuls les albums téléchargés sont disponibles.")
                },
                style = MaterialTheme.typography.bodyMedium,
                color = if (link == Link.Online) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.primary,
            )
            Text(
                tr("Adresses connues : ") + vm.repo.prefs.addresses.joinToString(", "),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                tr("Hors de chez toi, installe Tailscale sur le PC et sur le téléphone (même compte) : l'app retrouvera le PC toute seule."),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = { vm.repo.reconnect() }) { Text(tr("Reconnecter")) }
                OutlinedButton(onClick = { if (confirmUnpair) vm.repo.unpair() else confirmUnpair = true }) {
                    Text(if (confirmUnpair) tr("Confirmer") else tr("Délier ce téléphone"))
                }
            }
        }
        Section(tr("Qualité"))
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            var light by remember { mutableStateOf(vm.repo.prefs.lightOnMobile) }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(tr("Économiser les données mobiles"), style = MaterialTheme.typography.bodyLarge)
                    Text(
                        tr("En 4G, le PC envoie une version allégée (Opus 160 kbit/s, environ 8 fois plus légère). En Wi-Fi et pour les téléchargements : toujours le fichier d'origine."),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Switch(checked = light, onCheckedChange = { light = it; vm.repo.prefs.lightOnMobile = it })
            }
        }
        Section(tr("Hors ligne"))
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(tr("%.2f Go utilisés sur le téléphone").format(used / 1e9), style = MaterialTheme.typography.bodyMedium)
            OutlinedButton(
                onClick = {
                    if (confirmWipe) {
                        vm.downloads.removeAll()
                        confirmWipe = false
                        toast(tr("Téléchargements supprimés"))
                    } else confirmWipe = true
                },
                enabled = done.isNotEmpty(),
            ) { Text(if (confirmWipe) tr("Confirmer la suppression") else tr("Supprimer tous les téléchargements")) }
        }
        Section(tr("À propos"))
        Text(
            tr("MusicPi 1.0.0 · Les fichiers téléchargés sont les originaux (FLAC), ils restent dans l'espace privé de l'app."),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp),
        )
    }
}
