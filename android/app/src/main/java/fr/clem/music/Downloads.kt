package fr.clem.music

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import okhttp3.Request
import java.io.File
import java.security.MessageDigest

/**
 * Albums gardés sur le téléphone pour l'écoute hors ligne. Les fichiers sont copiés
 * tels quels (FLAC d'origine) dans le stockage privé de l'app.
 */
class Downloads(context: Context, private val repo: Repo) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val dir = File(context.filesDir, "musique").apply { mkdirs() }
    private val covers = File(context.filesDir, "pochettes").apply { mkdirs() }
    private val indexFile = File(context.filesDir, "telechargements.json")

    /** Chemin sur le PC → nom du fichier local. */
    private val files = HashMap<String, String>()

    private val _done = MutableStateFlow<Set<String>>(emptySet())
    /** Pistes disponibles hors ligne (chemins sur le PC). */
    val done: StateFlow<Set<String>> = _done

    /** Albums en cours de téléchargement → avancement (0..1). */
    private val _progress = MutableStateFlow<Map<String, Float>>(emptyMap())
    val progress: StateFlow<Map<String, Float>> = _progress

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error

    private val jobs = java.util.concurrent.ConcurrentHashMap<String, Job>()

    init {
        runCatching {
            files.putAll(json.decodeFromString(MapSerializer(String.serializer(), String.serializer()), indexFile.readText()))
        }
        files.entries.removeAll { !File(dir, it.value).exists() }
        _done.value = files.keys.toSet()
    }

    private fun hash(s: String): String =
        MessageDigest.getInstance("SHA-1").digest(s.toByteArray()).joinToString("") { "%02x".format(it) }.take(24)

    @Synchronized
    private fun save() {
        val tmp = File(indexFile.path + ".tmp")
        tmp.writeText(json.encodeToString(MapSerializer(String.serializer(), String.serializer()), HashMap(files)))
        tmp.renameTo(indexFile)
        _done.value = files.keys.toSet()
    }

    fun localFile(path: String): File? = files[path]?.let { File(dir, it) }?.takeIf { it.exists() }

    fun coverFile(album: Album): File? = File(covers, hash(album.key) + ".jpg").takeIf { it.exists() }

    fun isDownloaded(album: Album) = album.tracks.all { it.path in _done.value }

    fun download(album: Album) = downloadTracks(album.key, album.title, album.tracks, listOf(album))

    /**
     * Télécharge une liste de pistes (album ou playlist) ; `key` identifie le téléchargement
     * dans `progress`, `albums` sont les albums dont on garde la pochette.
     */
    fun downloadTracks(key: String, label: String, tracks: List<Track>, albums: List<Album>) {
        if (jobs[key]?.isActive == true) return
        _error.value = null
        jobs[key] = scope.launch {
            val todo = tracks.distinctBy { it.path }.filter { localFile(it.path) == null }
            val total = todo.sumOf { it.size.coerceAtLeast(1) }.toFloat().coerceAtLeast(1f)
            var got = 0L
            _progress.update { it + (key to 0f) }
            try {
                // Pochettes en bonne taille pour l'écran de lecture.
                for (album in albums) {
                    if (coverFile(album) == null) {
                        repo.coverUrl(album.first.path, 800)?.let { url ->
                            runCatching { fetch(url, File(covers, hash(album.key) + ".jpg")) {} }
                        }
                    }
                }
                for (t in todo) {
                    if (!isActive) break
                    val url = repo.fileUrl(t.path) ?: throw IllegalStateException(tr("PC injoignable"))
                    val ext = t.path.substringAfterLast('.', "flac").lowercase()
                    val name = hash(t.path) + "." + ext
                    fetch(url, File(dir, name)) { n ->
                        if (!isActive) throw kotlinx.coroutines.CancellationException()
                        got += n
                        _progress.update { it + (key to (got / total).coerceIn(0f, 1f)) }
                    }
                    synchronized(this@Downloads) { files[t.path] = name }
                    save()
                }
            } catch (e: kotlinx.coroutines.CancellationException) {
                // Annulé par l'utilisateur.
            } catch (e: Exception) {
                _error.value = (if (I18n.en) "Download interrupted: " else "Téléchargement interrompu : ") + "$label (${e.message ?: tr("réseau")})"
            } finally {
                _progress.update { it - key }
                jobs.remove(key)
            }
        }
    }

    fun cancel(album: Album) {
        cancel(album.key)
    }

    fun cancel(key: String) {
        jobs[key]?.cancel()
    }

    fun remove(album: Album) {
        cancel(album)
        removeTracks(album.tracks)
        File(covers, hash(album.key) + ".jpg").delete()
    }

    fun removeTracks(tracks: List<Track>) {
        synchronized(this) {
            for (t in tracks) files.remove(t.path)?.let { File(dir, it).delete() }
        }
        save()
    }

    fun removeAll() {
        jobs.values.forEach { it.cancel() }
        synchronized(this) { files.clear() }
        dir.listFiles()?.forEach { it.delete() }
        covers.listFiles()?.forEach { it.delete() }
        save()
    }

    fun usedBytes(): Long = (dir.listFiles()?.sumOf { it.length() } ?: 0L) + (covers.listFiles()?.sumOf { it.length() } ?: 0L)

    /** Téléchargement vers un fichier temporaire, renommé seulement une fois complet. */
    private fun fetch(url: String, dest: File, onBytes: (Long) -> Unit) {
        val tmp = File(dest.path + ".part")
        repo.client.newCall(Request.Builder().url(url).build()).execute().use { r ->
            if (!r.isSuccessful) throw IllegalStateException("erreur ${r.code}")
            r.body!!.byteStream().use { input ->
                tmp.outputStream().use { out ->
                    val buf = ByteArray(64 * 1024)
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        onBytes(n.toLong())
                    }
                }
            }
        }
        if (!tmp.renameTo(dest)) throw IllegalStateException(tr("écriture impossible"))
    }
}
