package fr.clem.music

import kotlinx.serialization.Serializable
import java.text.Normalizer

/** Piste, telle que le PC la décrit (même format que sa bibliothèque). */
@Serializable
data class Track(
    val path: String,
    val title: String = "",
    val artist: String = "",
    val album: String = "",
    val albumArtist: String = "",
    val track: Int? = null,
    val disc: Int? = null,
    val duration: Double = 0.0,
    val sampleRate: Int? = null,
    val bitDepth: Int? = null,
    val channels: Int? = null,
    val bitrate: Int? = null,
    val year: Int? = null,
    val genre: String = "",
    val size: Long = 0,
    val format: String = "",
) {
    val displayTitle: String get() = title.ifBlank { path.substringAfterLast('\\').substringAfterLast('/') }
    val mainArtist: String get() = albumArtist.ifBlank { artist }.ifBlank { "Artiste inconnu" }
    val quality: String
        get() = buildList {
            if (format.isNotBlank()) add(format.uppercase())
            if (bitDepth != null && sampleRate != null) add("$bitDepth bits / ${fmtRate(sampleRate)}")
            else if (bitrate != null) add("$bitrate kbit/s")
        }.joinToString(" · ")
}

private fun fmtRate(hz: Int): String {
    val k = hz / 1000.0
    return if (k % 1.0 == 0.0) "${k.toInt()} kHz" else "${"%.1f".format(k)} kHz"
}

/** Clé de regroupement : ignore la casse, les accents et la ponctuation (« Feu! Chatterton » = « FEU ! CHATTERTON »). */
fun nameKey(s: String): String =
    Normalizer.normalize(s, Normalizer.Form.NFD)
        .replace(MARKS, "")
        .lowercase()
        .replace(NOT_ALNUM, "")
        .ifEmpty { s.trim().lowercase() }

// Mêmes règles que le PC (lettres et chiffres Unicode, accents retirés).
private val MARKS = Regex("\\p{M}+")
private val NOT_ALNUM = Regex("[^\\p{L}\\p{N}]+")

data class Album(
    val key: String,
    val title: String,
    val artist: String,
    val artistKey: String,
    val year: Int?,
    val tracks: List<Track>,
) {
    val first: Track get() = tracks.first()
    val duration: Double get() = tracks.sumOf { it.duration }
    /** Clé d'album aimé, la même que sur le PC : « artiste|album ». */
    val likeKey: String get() = nameKey(artist) + "|" + nameKey(title)
}

data class Artist(
    val key: String,
    val name: String,
    val albums: List<Album>,
) {
    val trackCount: Int get() = albums.sumOf { it.tracks.size }
}

/** Index de la bibliothèque : artistes, albums, recherche. */
class LibraryIndex(val tracks: List<Track>) {
    val byPath: Map<String, Track> = tracks.associateBy { it.path }
    val albums: List<Album>
    val artists: List<Artist>
    private val albumOfPath = HashMap<String, Album>()

    init {
        // Nom affiché d'un artiste : l'orthographe la plus fréquente.
        val spellings = HashMap<String, HashMap<String, Int>>()
        for (t in tracks) {
            val n = t.mainArtist
            spellings.getOrPut(nameKey(n)) { HashMap() }.merge(n, 1, Int::plus)
        }
        val display = spellings.mapValues { (_, m) -> m.maxBy { it.value }.key }

        val groups = LinkedHashMap<String, MutableList<Track>>()
        for (t in tracks) {
            val k = nameKey(t.mainArtist) + "\u0000" + nameKey(t.album.ifBlank { "Album inconnu" })
            groups.getOrPut(k) { mutableListOf() }.add(t)
        }
        albums = groups.map { (k, list) ->
            val sorted = list.sortedWith(compareBy<Track>({ it.disc ?: 1 }, { it.track ?: 0 }, { it.path }))
            val ak = nameKey(sorted.first().mainArtist)
            Album(
                key = k,
                title = sorted.first().album.ifBlank { tr("Album inconnu") },
                artist = display[ak] ?: sorted.first().mainArtist,
                artistKey = ak,
                year = sorted.mapNotNull { it.year }.minOrNull(),
                tracks = sorted,
            )
        }.sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER) { it.title })
        for (a in albums) for (t in a.tracks) albumOfPath[t.path] = a

        artists = albums.groupBy { it.artistKey }.map { (k, list) ->
            Artist(k, display[k] ?: list.first().artist, list.sortedWith(compareBy({ it.year ?: 9999 }, { it.title.lowercase() })))
        }.sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER) { it.name.removePrefix("The ").removePrefix("the ") })
    }

    fun albumOf(path: String): Album? = albumOfPath[path]
    fun artist(key: String): Artist? = artists.firstOrNull { it.key == key }
    fun album(key: String): Album? = albums.firstOrNull { it.key == key }

    fun search(q: String): Triple<List<Artist>, List<Album>, List<Track>> {
        val n = searchNorm(q)
        val words = n.split(' ').filter { it.isNotEmpty() }
        if (words.isEmpty()) return Triple(emptyList(), emptyList(), emptyList())
        return Triple(
            rank(artists, { listOf(it.name) }, n, words, 20),
            rank(albums, { listOf(it.title, it.artist) }, n, words, 30),
            rank(tracks, { listOf(it.displayTitle, it.artist, it.album) }, n, words, 150),
        )
    }

    private val fieldCache = java.util.IdentityHashMap<Any, List<Pair<String, List<String>>>>()

    private fun <T : Any> rank(list: List<T>, fields: (T) -> List<String>, q: String, words: List<String>, max: Int): List<T> =
        list.asSequence()
            .map { x ->
                val f = synchronized(fieldCache) {
                    fieldCache.getOrPut(x) { fields(x).map { s -> searchNorm(s).let { it to it.split(' ') } } }
                }
                x to searchScore(f, q, words)
            }
            .filter { it.second > 0 }
            .sortedByDescending { it.second }
            .take(max)
            .map { it.first }
            .toList()
}

/** Recherche : sans accents ni casse ni ponctuation (mêmes règles que sur le PC). */
fun searchNorm(s: String): String =
    Normalizer.normalize(s, Normalizer.Form.NFD).replace(MARKS, "").lowercase().replace(NOT_ALNUM, " ").trim()

/** 0 = pas trouvé ; le premier champ (titre) compte double ; titre exact ou qui commence par la recherche en tête. */
private fun searchScore(fields: List<Pair<String, List<String>>>, q: String, words: List<String>): Double {
    var score = 0.0
    for (w in words) {
        var best = 0.0
        fields.forEachIndexed { i, (_, ws) ->
            val s = when {
                w in ws -> 3.0
                ws.any { it.startsWith(w) } -> 2.0
                fields[i].first.contains(w) -> 0.5
                else -> 0.0
            }
            best = maxOf(best, s * if (i == 0) 2 else 1)
        }
        if (best == 0.0) return 0.0
        score += best
    }
    val main = fields[0].first
    score += when {
        main == q -> 20.0
        main.startsWith(q) -> 10.0
        main.contains(" $q") -> 5.0
        else -> 0.0
    }
    return score
}

/** État du lecteur du PC (événements envoyés par le serveur). */
@Serializable
data class PcState(
    val ready: Boolean = false,
    val index: Int? = null,
    val path: String? = null,
    val playing: Boolean = false,
    val position: Double = 0.0,
    val duration: Double = 0.0,
    val volume: Double = 1.0,
    val error: String? = null,
    val practice: Boolean = false,
    val tempo: Double = 0.0,
    val tempoAvailable: Boolean = false,
    val loopRange: List<Double>? = null,
)

/** Playlist (chemins des fichiers sur le PC, dans l'ordre). */
@Serializable
data class Playlist(
    val id: String,
    val name: String = "",
    val tracks: List<String> = emptyList(),
    val created: Long = 0,
    val updated: Long = 0,
)

/** Cœurs et playlists, partagés avec le PC. */
@Serializable
data class Collections(
    val version: Long = 0,
    val likedTracks: List<String> = emptyList(),
    val likedAlbums: List<String> = emptyList(),
    val playlists: List<Playlist> = emptyList(),
)

@Serializable
data class ServerInfo(
    val app: String = "",
    val version: String = "",
    val name: String = "",
    val addresses: List<String> = emptyList(),
    val port: Int = 0,
)

fun fmtTime(sec: Double): String {
    val s = sec.coerceAtLeast(0.0).toLong()
    val h = s / 3600
    val m = (s % 3600) / 60
    val r = s % 60
    return if (h > 0) "%d:%02d:%02d".format(h, m, r) else "%d:%02d".format(m, r)
}

fun fmtPrecise(sec: Double): String {
    val m = (sec / 60).toInt()
    val r = sec - m * 60
    return "%d:%04.1f".format(m, r)
}
