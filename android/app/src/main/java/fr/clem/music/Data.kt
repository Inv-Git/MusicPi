package fr.clem.music

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.os.Build
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.sse.EventSource
import okhttp3.sse.EventSourceListener
import okhttp3.sse.EventSources
import java.io.File
import java.util.concurrent.TimeUnit

val json = Json { ignoreUnknownKeys = true; explicitNulls = false; coerceInputValues = true }

/** Réglages enregistrés sur le téléphone. */
class Prefs(context: Context) {
    private val p = context.getSharedPreferences("music", Context.MODE_PRIVATE)

    var token: String
        get() = p.getString("token", "") ?: ""
        set(v) = p.edit().putString("token", v).apply()

    var pcName: String
        get() = p.getString("pcName", "") ?: ""
        set(v) = p.edit().putString("pcName", v).apply()

    /** Adresses « ip:port » du PC, la dernière qui a marché en premier. */
    var addresses: List<String>
        get() = (p.getString("addresses", "") ?: "").split('\n').filter { it.isNotBlank() }
        set(v) = p.edit().putString("addresses", v.distinct().joinToString("\n")).apply()

    var libraryEtag: String
        get() = p.getString("etag", "") ?: ""
        set(v) = p.edit().putString("etag", v).apply()

    /** En données mobiles : version allégée (Opus) plutôt que le fichier d'origine. */
    var lightOnMobile: Boolean
        get() = p.getBoolean("lightOnMobile", true)
        set(v) = p.edit().putBoolean("lightOnMobile", v).apply()

    /** Où joue la musique : « phone » ou « pc ». */
    var target: String
        get() = p.getString("target", "phone") ?: "phone"
        set(v) = p.edit().putString("target", v).apply()

    val paired: Boolean get() = token.isNotBlank() && addresses.isNotEmpty()

    fun forget() = p.edit().remove("token").remove("addresses").remove("pcName").remove("etag").apply()
}

enum class Link { Offline, Connecting, Online }

/**
 * Accès au PC : trouve une adresse qui répond (maison ou Tailscale), récupère la
 * bibliothèque, écoute l'état du lecteur et envoie les commandes.
 */
class Repo(private val context: Context, val prefs: Prefs) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    val client: OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(4, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()
    private val sseClient = client.newBuilder().readTimeout(45, TimeUnit.SECONDS).build()
    private val probeClient = client.newBuilder().connectTimeout(2, TimeUnit.SECONDS).readTimeout(3, TimeUnit.SECONDS).build()

    @Volatile var base: HttpUrl? = null
        private set

    private val _link = MutableStateFlow(Link.Offline)
    val link: StateFlow<Link> = _link

    private val _index = MutableStateFlow<LibraryIndex?>(null)
    val index: StateFlow<LibraryIndex?> = _index

    private val _paired = MutableStateFlow(prefs.paired)
    val paired: StateFlow<Boolean> = _paired

    private val _pc = MutableStateFlow<PcState?>(null)
    val pc: StateFlow<PcState?> = _pc

    private val libFile = File(context.filesDir, "library.json")
    private val collFile = File(context.filesDir, "collections.json")
    private val pendingFile = File(context.filesDir, "collections-attente.json")

    /** Cœurs et playlists (copie locale, disponible hors ligne). */
    private val _coll = MutableStateFlow(
        runCatching { json.decodeFromString(Collections.serializer(), collFile.readText()) }.getOrDefault(Collections()),
    )
    val collections: StateFlow<Collections> = _coll

    /** Modifications faites sur le téléphone, pas encore reçues par le PC (hors ligne). */
    private val pending: MutableList<JsonObject> = runCatching {
        json.decodeFromString(ListSerializer(JsonObject.serializer()), pendingFile.readText()).toMutableList()
    }.getOrDefault(mutableListOf())
    private val flushLock = Mutex()
    private var events: EventSource? = null
    private var connectJob: Job? = null

    init {
        // Bibliothèque gardée sur le téléphone : affichée tout de suite, même sans réseau.
        scope.launch { loadCachedLibrary() }
        watchNetwork()
    }

    private fun loadCachedLibrary() {
        if (_index.value != null || !libFile.exists()) return
        runCatching { json.decodeFromString(ListSerializer(Track.serializer()), libFile.readText()) }
            .onSuccess { _index.value = LibraryIndex(it) }
    }

    private fun watchNetwork() {
        val cm = context.getSystemService(ConnectivityManager::class.java) ?: return
        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) { reconnect() }
            override fun onLost(network: Network) { reconnect() }
        }
        runCatching { cm.registerDefaultNetworkCallback(cb) }
    }

    /** (Re)cherche le PC puis recharge la bibliothèque et l'état du lecteur. */
    fun reconnect() {
        if (!prefs.paired) return
        connectJob?.cancel()
        connectJob = scope.launch {
            _link.value = Link.Connecting
            var wait = 2_000L
            while (true) {
                val b = findServer()
                if (b != null) {
                    base = b
                    _link.value = Link.Online
                    refreshLibrary()
                    flushPending()
                    flushPlayed()
                    listenEvents()
                    return@launch
                }
                base = null
                _link.value = Link.Offline
                delay(wait)
                wait = (wait * 2).coerceAtMost(60_000L)
            }
        }
    }

    private suspend fun findServer(): HttpUrl? = withContext(Dispatchers.IO) {
        val candidates = prefs.addresses.mapNotNull { "http://$it/".toHttpUrlOrNull() }
        val found = candidates.map { u ->
            async {
                runCatching {
                    probeClient.newCall(Request.Builder().url(u.resolve("api/info")!!).build()).execute().use { r ->
                        if (!r.isSuccessful) null else u to json.decodeFromString(ServerInfo.serializer(), r.body!!.string())
                    }
                }.getOrNull()
            }
        }.awaitAll().filterNotNull()
        // À la maison, on préfère l'adresse locale à Tailscale.
        val (u, info) = found.sortedBy { it.first.host.startsWith("100.") }.firstOrNull() ?: return@withContext null
        // Mémorise les adresses annoncées par le PC (ex. Tailscale installé après l'appairage).
        val learned = info.addresses.map { "$it:${info.port}" }
        prefs.addresses = listOf("${u.host}:${u.port}") + prefs.addresses + learned
        if (info.name.isNotBlank()) prefs.pcName = info.name
        u
    }

    /** Premier contact : échange le code affiché sur le PC contre un jeton. */
    suspend fun pair(address: String, code: String): String? = withContext(Dispatchers.IO) {
        val addr = address.trim().removePrefix("http://").trimEnd('/').let { if (':' in it) it else "$it:47800" }
        val u = "http://$addr/".toHttpUrlOrNull() ?: return@withContext "Adresse invalide."
        val body = buildJsonObject {
            put("code", code.filter { it.isDigit() })
            put("device", "${Build.MANUFACTURER.replaceFirstChar { it.uppercase() }} ${Build.MODEL}")
        }.toString().toRequestBody(JSON)
        try {
            client.newCall(Request.Builder().url(u.resolve("api/pair")!!).post(body).build()).execute().use { r ->
                val txt = r.body?.string().orEmpty()
                val obj = runCatching { json.parseToJsonElement(txt).jsonObject }.getOrNull()
                if (!r.isSuccessful) return@withContext obj?.get("error")?.jsonPrimitive?.content?.let(::tr) ?: (if (I18n.en) "Error ${r.code}" else "Erreur ${r.code}")
                prefs.token = obj?.get("token")?.jsonPrimitive?.content ?: return@withContext tr("Réponse inattendue.")
                prefs.pcName = obj["name"]?.jsonPrimitive?.content ?: ""
                prefs.addresses = listOf(addr)
                prefs.libraryEtag = ""
            }
            _paired.value = true
        } catch (e: Exception) {
            return@withContext tr("PC injoignable. Vérifie l'adresse, que le partage est activé dans Paramètres › Téléphone, et que le pare-feu Windows l'autorise.")
        }
        reconnect()
        null
    }

    fun unpair() {
        events?.cancel()
        connectJob?.cancel()
        prefs.forget()
        _paired.value = false
        base = null
        _pc.value = null
        _link.value = Link.Offline
    }

    private fun authed(url: HttpUrl) = Request.Builder().url(url).header("Authorization", "Bearer ${prefs.token}")

    private fun onUnauthorized(r: Response) {
        if (r.code == 401) {
            // Appareil retiré depuis le PC.
            scope.launch(Dispatchers.Main) { unpair() }
        }
    }

    suspend fun refreshLibrary() = withContext(Dispatchers.IO) {
        val b = base ?: return@withContext
        loadCachedLibrary()
        val req = authed(b.resolve("api/library")!!).apply {
            if (prefs.libraryEtag.isNotBlank() && libFile.exists()) header("If-None-Match", prefs.libraryEtag)
        }.build()
        runCatching {
            client.newCall(req).execute().use { r ->
                onUnauthorized(r)
                if (r.code == 304 || !r.isSuccessful) return@use
                val txt = r.body!!.string()
                val list = json.decodeFromString(ListSerializer(Track.serializer()), txt)
                val tmp = File(libFile.path + ".tmp")
                tmp.writeText(txt)
                tmp.renameTo(libFile)
                prefs.libraryEtag = r.header("ETag") ?: ""
                _index.value = LibraryIndex(list)
            }
        }
    }

    private fun listenEvents() {
        events?.cancel()
        val b = base ?: return
        val req = authed(b.resolve("api/events")!!).header("Accept", "text/event-stream").build()
        events = EventSources.createFactory(sseClient).newEventSource(req, object : EventSourceListener() {
            override fun onEvent(eventSource: EventSource, id: String?, type: String?, data: String) {
                if (type == "collections") {
                    runCatching { json.decodeFromString(Collections.serializer(), data) }.onSuccess { fromServer(it) }
                    return
                }
                runCatching { json.decodeFromString(PcState.serializer(), data) }.onSuccess { _pc.value = it }
            }

            override fun onFailure(eventSource: EventSource, t: Throwable?, response: Response?) {
                response?.let { onUnauthorized(it) }
                if (eventSource == events) {
                    _pc.value = null
                    reconnect()
                }
            }

            override fun onClosed(eventSource: EventSource) {
                if (eventSource == events) reconnect()
            }
        })
    }

    /** Commande envoyée au lecteur du PC. */
    fun cmd(body: JsonObject) {
        val b = base ?: return
        scope.launch {
            runCatching {
                client.newCall(authed(b.resolve("api/cmd")!!).post(body.toString().toRequestBody(JSON)).build()).execute().use { onUnauthorized(it) }
            }
        }
    }

    // --- cœurs et playlists ---------------------------------------------------------

    private fun saveColl(c: Collections) {
        runCatching {
            val tmp = File(collFile.path + ".tmp")
            tmp.writeText(json.encodeToString(Collections.serializer(), c))
            tmp.renameTo(collFile)
        }
    }

    private fun savePending() {
        runCatching { pendingFile.writeText(json.encodeToString(ListSerializer(JsonObject.serializer()), pending)) }
    }

    /** Version du PC, plus les modifications du téléphone qu'il n'a pas encore reçues. */
    private fun fromServer(c: Collections) {
        val waiting = synchronized(pending) { pending.toList() }
        val merged = waiting.fold(c) { acc, op -> Ops.apply(acc, op) }
        _coll.value = merged
        saveColl(merged)
    }

    /** Modification faite sur le téléphone : appliquée tout de suite, envoyée au PC dès que possible. */
    fun collOp(op: JsonObject) {
        val c = Ops.apply(_coll.value, op)
        _coll.value = c
        saveColl(c)
        synchronized(pending) {
            pending.add(op)
            savePending()
        }
        scope.launch { flushPending() }
    }

    private suspend fun flushPending(): Unit = flushLock.withLock {
        val b = base ?: return@withLock
        var last: Collections? = null
        while (true) {
            val op = synchronized(pending) { pending.firstOrNull() } ?: break
            val req = authed(b.resolve("api/collections")!!).post(op.toString().toRequestBody(JSON)).build()
            val ok = runCatching {
                client.newCall(req).execute().use { r ->
                    onUnauthorized(r)
                    when {
                        r.isSuccessful -> {
                            val obj = json.parseToJsonElement(r.body!!.string()).jsonObject
                            last = json.decodeFromJsonElement(Collections.serializer(), obj["collections"]!!)
                            true
                        }
                        // Refusée par le PC (playlist supprimée entre-temps…) : on l'abandonne.
                        r.code in 400..499 && r.code != 401 -> true
                        else -> false
                    }
                }
            }.getOrDefault(false)
            if (!ok) return@withLock
            synchronized(pending) {
                if (pending.isNotEmpty()) pending.removeAt(0)
                savePending()
            }
        }
        last?.let { fromServer(it) }
    }

    private fun withToken(path: String, vararg q: Pair<String, String>): String? {
        val b = base ?: return null
        val u = b.resolve(path)!!.newBuilder()
        for ((k, v) in q) u.addQueryParameter(k, v)
        u.addQueryParameter("t", prefs.token)
        return u.build().toString()
    }

    /** Adresse du fichier ; `stream` = écoute en direct (version allégée possible en 4G). */
    fun fileUrl(path: String, stream: Boolean = false): String? {
        val light = stream && prefs.lightOnMobile && metered()
        return if (light) withToken("api/file", "p" to path, "q" to "low") else withToken("api/file", "p" to path)
    }

    /** Réseau facturé au volume (4G/5G, partage de connexion…). */
    fun metered(): Boolean =
        runCatching { context.getSystemService(ConnectivityManager::class.java)?.isActiveNetworkMetered ?: false }.getOrDefault(false)

    // --- écoutes faites sur le téléphone (comptées sur le PC, et Last.fm) -------------

    private val playedFile = File(context.filesDir, "ecoutes-attente.json")

    // --- Paroles -------------------------------------------------------------------------------

    private val lyricsDir = File(context.filesDir, "paroles")

    private fun lyricsFile(path: String): File {
        var h = -0x340d631b7bdddcdbL
        for (b in path.toByteArray()) { h = h xor (b.toLong() and 0xff); h *= 0x100000001b3L }
        return File(lyricsDir, java.lang.Long.toHexString(h) + ".json")
    }

    /** Paroles d'un morceau : demandées au PC, gardées sur le téléphone pour le hors ligne. */
    suspend fun lyrics(path: String): LyricsOut? = withContext(Dispatchers.IO) {
        val f = lyricsFile(path)
        val b = base
        if (b != null) {
            val got = runCatching {
                val url = b.resolve("api/lyrics")!!.newBuilder().addQueryParameter("p", path).build()
                client.newCall(authed(url).build()).execute().use { r ->
                    onUnauthorized(r)
                    if (!r.isSuccessful) return@use null
                    val txt = r.body!!.string()
                    lyricsDir.mkdirs()
                    f.writeText(txt)
                    json.decodeFromString(LyricsOut.serializer(), txt)
                }
            }.getOrNull()
            if (got != null) return@withContext got
        }
        if (f.exists()) runCatching { json.decodeFromString(LyricsOut.serializer(), f.readText()) }.getOrNull() else null
    }
    private val playedQueue: MutableList<String> = runCatching {
        json.decodeFromString(ListSerializer(String.serializer()), playedFile.readText()).toMutableList()
    }.getOrDefault(mutableListOf())

    fun reportPlayed(path: String) {
        synchronized(playedQueue) {
            playedQueue.add(path)
            runCatching { playedFile.writeText(json.encodeToString(ListSerializer(String.serializer()), playedQueue)) }
        }
        scope.launch { flushPlayed() }
    }

    private fun flushPlayed() {
        val b = base ?: return
        while (true) {
            val path = synchronized(playedQueue) { playedQueue.firstOrNull() } ?: break
            val body = buildJsonObject { put("path", path) }.toString().toRequestBody(JSON)
            val ok = runCatching {
                client.newCall(authed(b.resolve("api/played")!!).post(body).build()).execute().use { it.isSuccessful || it.code == 404 }
            }.getOrDefault(false)
            if (!ok) return
            synchronized(playedQueue) {
                if (playedQueue.isNotEmpty()) playedQueue.removeAt(0)
                runCatching { playedFile.writeText(json.encodeToString(ListSerializer(String.serializer()), playedQueue)) }
            }
        }
    }
    fun coverUrl(path: String, size: Int = 400) = withToken("api/cover", "p" to path, "s" to size.toString())
    fun artistUrl(name: String, track: String, size: Int = 400) = withToken("api/artist", "n" to name, "t" to track, "s" to size.toString())

    companion object {
        val JSON = "application/json; charset=utf-8".toMediaType()
    }
}
