package fr.clem.music

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.util.UUID

/** Modifications des cœurs et playlists : même format que celles envoyées au PC. */
object Ops {
    fun likeTrack(path: String, on: Boolean) = buildJsonObject { put("op", "likeTrack"); put("path", path); put("on", on) }
    fun likeAlbum(key: String, on: Boolean) = buildJsonObject { put("op", "likeAlbum"); put("key", key); put("on", on) }
    fun create(id: String, name: String, paths: List<String>) = buildJsonObject {
        put("op", "create"); put("id", id); put("name", name); put("paths", JsonArray(paths.map { JsonPrimitive(it) }))
    }
    fun rename(id: String, name: String) = buildJsonObject { put("op", "rename"); put("id", id); put("name", name) }
    fun delete(id: String) = buildJsonObject { put("op", "delete"); put("id", id) }
    fun add(id: String, paths: List<String>) = buildJsonObject { put("op", "add"); put("id", id); put("paths", JsonArray(paths.map { JsonPrimitive(it) })) }
    fun remove(id: String, paths: List<String>) = buildJsonObject { put("op", "remove"); put("id", id); put("paths", JsonArray(paths.map { JsonPrimitive(it) })) }
    fun move(id: String, from: Int, to: Int) = buildJsonObject { put("op", "move"); put("id", id); put("from", from); put("to", to) }

    /** Identifiant créé sur le téléphone, pour pouvoir remplir une playlist même hors ligne. */
    fun newId() = "t" + UUID.randomUUID().toString().replace("-", "").take(15)

    /**
     * Applique une modification sur la copie du téléphone, en attendant la réponse du PC
     * (qui fait foi). Même logique que le PC.
     */
    fun apply(c: Collections, op: JsonObject, now: Long = System.currentTimeMillis() / 1000): Collections {
        fun str(k: String) = op[k]?.jsonPrimitive?.contentOrNull
        val paths = runCatching { op["paths"]?.jsonArray?.mapNotNull { it.jsonPrimitive.contentOrNull } }.getOrNull().orEmpty()
        val on = op["on"]?.jsonPrimitive?.booleanOrNull ?: false
        val id = str("id") ?: ""
        fun flag(list: List<String>, item: String?): List<String> = when {
            item == null -> list
            on && item !in list -> listOf(item) + list
            !on -> list.filter { it != item }
            else -> list
        }
        fun edit(f: (Playlist) -> Playlist): Collections {
            if (c.playlists.none { it.id == id }) return c
            return c.copy(version = c.version + 1, playlists = c.playlists.map { if (it.id == id) f(it).copy(updated = now) else it })
        }
        return when (str("op")) {
            "likeTrack" -> flag(c.likedTracks, str("path")).let { if (it == c.likedTracks) c else c.copy(version = c.version + 1, likedTracks = it) }
            "likeAlbum" -> flag(c.likedAlbums, str("key")).let { if (it == c.likedAlbums) c else c.copy(version = c.version + 1, likedAlbums = it) }
            "create" -> if (id.isEmpty() || c.playlists.any { it.id == id }) c else {
                val name = (str("name") ?: "").trim().ifEmpty { tr("Nouvelle playlist") }
                c.copy(version = c.version + 1, playlists = listOf(Playlist(id, name, paths, now, now)) + c.playlists)
            }
            "rename" -> (str("name") ?: "").trim().let { n -> if (n.isEmpty()) c else edit { it.copy(name = n) } }
            "delete" -> if (c.playlists.none { it.id == id }) c else c.copy(version = c.version + 1, playlists = c.playlists.filter { it.id != id })
            "add" -> edit { it.copy(tracks = it.tracks + paths) }
            "remove" -> edit { p -> p.copy(tracks = p.tracks.filter { it !in paths }) }
            "move" -> {
                val from = op["from"]?.jsonPrimitive?.intOrNull ?: return c
                val to = op["to"]?.jsonPrimitive?.intOrNull ?: return c
                edit { p ->
                    if (from !in p.tracks.indices) p else {
                        val l = p.tracks.toMutableList()
                        val t = l.removeAt(from)
                        l.add(to.coerceIn(0, l.size), t)
                        p.copy(tracks = l)
                    }
                }
            }
            else -> c
        }
    }
}
