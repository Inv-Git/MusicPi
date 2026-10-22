package fr.clem.music

import android.app.Application
import android.content.ComponentName
import android.net.Uri
import androidx.core.content.ContextCompat
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

enum class Target { Phone, Pc }

/** Ce qui est en cours de lecture, où que ce soit (téléphone ou PC). */
data class Now(
    val target: Target = Target.Phone,
    val path: String? = null,
    val playing: Boolean = false,
    val position: Double = 0.0,
    val duration: Double = 0.0,
    /** Vitesse en % (100 = normale). */
    val speed: Int = 100,
    val speedAvailable: Boolean = true,
    val loop: Pair<Double, Double>? = null,
    /** Volume du PC (0..1), seulement en mode télécommande. */
    val volume: Double? = null,
    val reachable: Boolean = true,
)

private data class PhoneSnap(val path: String?, val playing: Boolean, val position: Double, val duration: Double, val speed: Float)

class MainViewModel(app: Application) : AndroidViewModel(app) {
    private val music = app as MusicApp
    val repo = music.repo
    val downloads = music.downloads

    private var controller: MediaController? = null
    private val phone = MutableStateFlow(PhoneSnap(null, false, 0.0, 0.0, 1f))
    private val target = MutableStateFlow(if (repo.prefs.target == "pc") Target.Pc else Target.Phone)

    /** Début de boucle posé, en attente de la fin. */
    private val pendingA = MutableStateFlow<Double?>(null)
    val loopStart: StateFlow<Double?> = pendingA

    val now: StateFlow<Now> = combine(target, phone, repo.pc, PhoneLoop.range, repo.link) { tg, ph, pc, loop, link ->
        when (tg) {
            Target.Phone -> Now(Target.Phone, ph.path, ph.playing, ph.position, ph.duration, (ph.speed * 100).toInt().coerceIn(50, 150), true, loop)
            Target.Pc -> Now(
                target = Target.Pc,
                path = pc?.path,
                playing = pc?.playing ?: false,
                position = pc?.position ?: 0.0,
                duration = pc?.duration ?: 0.0,
                speed = pc?.let { if (it.practice) (100 + it.tempo).toInt() else 100 } ?: 100,
                speedAvailable = pc?.tempoAvailable ?: false,
                loop = pc?.loopRange?.takeIf { it.size == 2 }?.let { it[0] to it[1] },
                volume = pc?.volume,
                reachable = link == Link.Online && pc != null,
            )
        }
    }.stateIn(viewModelScope, SharingStarted.Eagerly, Now())

    private val listener = object : Player.Listener {
        override fun onEvents(player: Player, events: Player.Events) = snap()
    }

    init {
        val token = SessionToken(app, ComponentName(app, PlaybackService::class.java))
        val future = MediaController.Builder(app, token).buildAsync()
        future.addListener({
            runCatching { future.get() }.onSuccess {
                controller = it
                it.addListener(listener)
                snap()
            }
        }, ContextCompat.getMainExecutor(app))
        // Position qui avance pendant la lecture.
        viewModelScope.launch {
            while (true) {
                delay(250)
                if (controller?.isPlaying == true) {
                    snap()
                    countListening()
                }
            }
        }
        // Nouvelle piste : on oublie le début de boucle en attente.
        viewModelScope.launch {
            var last: String? = null
            now.collect { if (it.path != last) { last = it.path; pendingA.value = null } }
        }
    }

    // Écoute sur le téléphone : comptée à la moitié du titre (ou 4 min), comme sur le PC.
    private var heardPath: String? = null
    private var heardSecs = 0.0
    private var heardLast = 0.0
    private var heardDone = false

    private fun countListening() {
        val ph = phone.value
        val path = ph.path ?: return
        if (target.value != Target.Phone) return
        if (path != heardPath) {
            heardPath = path; heardSecs = 0.0; heardLast = ph.position; heardDone = false
        }
        val d = ph.position - heardLast
        if (d < -5 && ph.position < 3) { heardSecs = 0.0; heardDone = false } else if (d > 0 && d < 3) heardSecs += d
        heardLast = ph.position
        val need = (ph.duration / 2).coerceIn(30.0, 240.0)
        if (!heardDone && heardSecs >= need) {
            heardDone = true
            repo.reportPlayed(path)
        }
    }

    private fun snap() {
        val c = controller ?: return
        val d = c.duration
        phone.value = PhoneSnap(
            path = c.currentMediaItem?.mediaId,
            playing = c.isPlaying,
            position = c.currentPosition / 1000.0,
            duration = if (d > 0) d / 1000.0 else (c.currentMediaItem?.mediaId?.let { repo.index.value?.byPath?.get(it)?.duration } ?: 0.0),
            speed = c.playbackParameters.speed,
        )
    }

    override fun onCleared() {
        controller?.removeListener(listener)
        controller?.release()
    }

    // --- lecture ---------------------------------------------------------------------

    /** Une piste est jouable sur le téléphone si elle est téléchargée ou si le PC répond. */
    fun playableOnPhone(t: Track) = downloads.localFile(t.path) != null || repo.link.value == Link.Online

    private fun item(t: Track): MediaItem? {
        val uri = downloads.localFile(t.path)?.let { Uri.fromFile(it) } ?: repo.fileUrl(t.path, stream = true)?.let { Uri.parse(it) } ?: return null
        val album = repo.index.value?.albumOf(t.path)
        val art = album?.let { downloads.coverFile(it) }?.let { Uri.fromFile(it) } ?: repo.coverUrl(t.path, 600)?.let { Uri.parse(it) }
        return MediaItem.Builder()
            .setMediaId(t.path)
            .setUri(uri)
            .setRequestMetadata(MediaItem.RequestMetadata.Builder().setMediaUri(uri).build())
            .setMediaMetadata(
                MediaMetadata.Builder()
                    .setTitle(t.displayTitle)
                    .setArtist(t.artist)
                    .setAlbumTitle(t.album)
                    .setAlbumArtist(t.albumArtist)
                    .setArtworkUri(art)
                    .build(),
            )
            .build()
    }

    fun play(tracks: List<Track>, index: Int, startAt: Double = 0.0) {
        if (tracks.isEmpty()) return
        when (target.value) {
            Target.Pc -> {
                repo.cmd(buildJsonObject {
                    put("cmd", "play")
                    put("paths", JsonArray(tracks.map { JsonPrimitive(it.path) }))
                    put("index", index)
                })
                if (startAt > 0) seek(startAt)
            }
            Target.Phone -> {
                val c = controller ?: return
                val chosen = tracks[index]
                val items = tracks.filter { playableOnPhone(it) }.mapNotNull { t -> item(t)?.let { t to it } }
                val i = items.indexOfFirst { it.first.path == chosen.path }.coerceAtLeast(0)
                if (items.isEmpty()) return
                PhoneLoop.range.value = null
                c.setMediaItems(items.map { it.second }, i, (startAt * 1000).toLong())
                c.prepare()
                c.play()
            }
        }
    }

    fun toggle() = when (target.value) {
        Target.Pc -> repo.cmd(buildJsonObject { put("cmd", "toggle") })
        Target.Phone -> controller?.let { if (it.isPlaying) it.pause() else { if (it.playbackState == Player.STATE_IDLE) it.prepare(); it.play() } } ?: Unit
    }

    fun next() = when (target.value) {
        Target.Pc -> repo.cmd(buildJsonObject { put("cmd", "next") })
        Target.Phone -> controller?.seekToNextMediaItem() ?: Unit
    }

    fun prev() = when (target.value) {
        Target.Pc -> repo.cmd(buildJsonObject { put("cmd", "prev") })
        Target.Phone -> controller?.seekToPrevious() ?: Unit
    }

    // --- Paroles ---------------------------------------------------------------------------------

    data class LyricsState(val path: String, val loading: Boolean, val out: LyricsOut?)

    private val _lyrics = MutableStateFlow<LyricsState?>(null)
    val lyrics: StateFlow<LyricsState?> = _lyrics

    fun loadLyrics(path: String) {
        val cur = _lyrics.value
        if (cur != null && cur.path == path && (cur.loading || cur.out != null)) return
        _lyrics.value = LyricsState(path, true, null)
        viewModelScope.launch {
            val r = repo.lyrics(path)
            if (_lyrics.value?.path == path) _lyrics.value = LyricsState(path, false, r ?: LyricsOut())
        }
    }

    fun seek(sec: Double) = when (target.value) {
        Target.Pc -> repo.cmd(buildJsonObject { put("cmd", "seek"); put("position", sec) })
        Target.Phone -> controller?.seekTo((sec * 1000).toLong()) ?: Unit
    }

    fun setVolume(v: Double) = repo.cmd(buildJsonObject { put("cmd", "volume"); put("volume", v) })

    /** Dernier volume envoyé au PC et quand (pour enchaîner les appuis sans attendre sa réponse). */
    private var volSent = 1.0
    private var volSentAt = 0L
    /** Volume du PC affiché brièvement après un appui sur les touches du téléphone. */
    val volumeFlash = MutableStateFlow<Pair<Double, Long>?>(null)

    /**
     * Touches volume du téléphone : en mode télécommande, elles règlent le PC.
     * Renvoie `false` pour laisser le téléphone gérer son propre volume.
     */
    fun volumeKey(up: Boolean): Boolean {
        val n = now.value
        if (n.target != Target.Pc || !n.reachable) return false
        val t = System.currentTimeMillis()
        val base = if (t - volSentAt < 1500) volSent else (n.volume ?: 1.0)
        val v = ((base + if (up) 0.05 else -0.05) * 20).let { Math.round(it) / 20.0 }.coerceIn(0.0, 1.0)
        volSent = v
        volSentAt = t
        setVolume(v)
        volumeFlash.value = v to t
        return true
    }

    // --- mode travail ------------------------------------------------------------------

    fun setSpeed(pct: Int) {
        val p = pct.coerceIn(50, 150)
        when (target.value) {
            Target.Pc -> {
                if (now.value.speed == 100 && p != 100) repo.cmd(buildJsonObject { put("cmd", "practice"); put("on", true) })
                repo.cmd(buildJsonObject { put("cmd", "tempo"); put("tempo", p - 100) })
            }
            Target.Phone -> controller?.setPlaybackSpeed(p / 100f)
        }
    }

    private fun sendLoop(r: Pair<Double, Double>?) {
        when (target.value) {
            Target.Pc -> repo.cmd(buildJsonObject {
                put("cmd", "loop")
                if (r != null) { put("start", r.first); put("end", r.second) } else { put("start", JsonNull); put("end", JsonNull) }
            })
            Target.Phone -> PhoneLoop.range.value = r
        }
    }

    fun markLoopStart() {
        val n = now.value
        if (n.path == null) return
        val cur = n.loop
        pendingA.value = n.position
        // Nouvelle entrée avant la sortie existante : on garde la sortie.
        if (cur != null && n.position < cur.second - 0.2) sendLoop(n.position to cur.second) else if (cur != null) sendLoop(null)
    }

    /** Pose la fin de la boucle ; renvoie un message si ce n'est pas possible. */
    fun markLoopEnd(): String? {
        val n = now.value
        if (n.path == null) return null
        val a = pendingA.value ?: n.loop?.first ?: 0.0
        if (n.position <= a + 0.2) return tr("La fin de la boucle doit être après son début.")
        sendLoop(a to n.position)
        pendingA.value = null
        seek(a)
        return null
    }

    fun clearLoop() {
        pendingA.value = null
        sendLoop(null)
    }

    // --- cœurs et playlists ------------------------------------------------------------

    val collections = repo.collections

    fun toggleLike(path: String) = repo.collOp(Ops.likeTrack(path, path !in collections.value.likedTracks))

    fun toggleLikeAlbum(album: Album) = repo.collOp(Ops.likeAlbum(album.likeKey, album.likeKey !in collections.value.likedAlbums))

    /** Crée une playlist (même hors ligne) et renvoie son identifiant. */
    fun createPlaylist(name: String, paths: List<String> = emptyList()): String {
        val id = Ops.newId()
        repo.collOp(Ops.create(id, name, paths))
        return id
    }

    fun addToPlaylist(id: String, paths: List<String>) = repo.collOp(Ops.add(id, paths))
    fun removeFromPlaylist(id: String, paths: List<String>) = repo.collOp(Ops.remove(id, paths))
    fun renamePlaylist(id: String, name: String) = repo.collOp(Ops.rename(id, name))
    fun deletePlaylist(id: String) = repo.collOp(Ops.delete(id))
    fun movePlaylistTrack(id: String, from: Int, to: Int) = repo.collOp(Ops.move(id, from, to))

    /** Pistes d'une playlist (« liked » = titres aimés) présentes dans la bibliothèque. */
    fun playlistTracks(id: String): List<Track> {
        val idx = repo.index.value ?: return emptyList()
        val c = collections.value
        val paths = if (id == "liked") c.likedTracks else c.playlists.firstOrNull { it.id == id }?.tracks.orEmpty()
        return paths.mapNotNull { idx.byPath[it] }
    }

    fun downloadPlaylist(id: String, name: String) {
        val idx = repo.index.value ?: return
        val tracks = playlistTracks(id)
        val albums = tracks.mapNotNull { idx.albumOf(it.path) }.distinctBy { it.key }
        downloads.downloadTracks("pl:$id", name, tracks, albums)
    }

    // --- où joue la musique ------------------------------------------------------------

    /** Bascule la lecture en cours vers l'autre appareil, au même endroit. */
    fun switchTarget(to: Target) {
        if (to == target.value) return
        val n = now.value
        val idx = repo.index.value
        when (to) {
            Target.Pc -> {
                val c = controller
                val paths = c?.let { (0 until it.mediaItemCount).map { i -> it.getMediaItemAt(i).mediaId } }.orEmpty()
                val wasPlaying = c?.isPlaying == true
                c?.pause()
                target.value = Target.Pc
                repo.prefs.target = "pc"
                if (paths.isNotEmpty() && wasPlaying) {
                    val tracks = paths.mapNotNull { idx?.byPath?.get(it) }
                    val i = tracks.indexOfFirst { it.path == n.path }.coerceAtLeast(0)
                    play(tracks, i, n.position)
                }
            }
            Target.Phone -> {
                val pc = repo.pc.value
                target.value = Target.Phone
                repo.prefs.target = "phone"
                if (pc?.playing == true && pc.path != null) {
                    repo.cmd(buildJsonObject { put("cmd", "toggle") })
                    val album = idx?.albumOf(pc.path)
                    val tracks = album?.tracks ?: listOfNotNull(idx?.byPath?.get(pc.path))
                    val i = tracks.indexOfFirst { it.path == pc.path }.coerceAtLeast(0)
                    play(tracks, i, pc.position)
                }
            }
        }
    }
}
