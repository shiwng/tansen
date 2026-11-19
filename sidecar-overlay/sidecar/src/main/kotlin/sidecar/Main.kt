/*
 * BitChord sidecar — a local HTTP server over :shared's YouTube Music client.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 * Part of BitChord (https://github.com/kushagrasinghx/BitChord), GPL-3.0.
 *
 * Design rules:
 *  - Binds 127.0.0.1 only. This is a private helper for the desktop app on
 *    the same machine, not a service.
 *  - Nothing in :shared is changed. Every seam lives in this module: the
 *    mime/itag enrichment for the stream endpoint is done at the adapter
 *    level (URL parameter + one ranged GET), not inside StreamResolver.
 *  - Each request logs one line to stdout; every failure logs its reason.
 *    SIGTERM stops the server and exits 0.
 */

package sidecar

import com.music.bitchord.data.YtMusicRepository
import com.music.bitchord.data.innertube.StreamResolver
import com.music.bitchord.data.model.SearchFilter
import com.music.bitchord.data.model.SearchResult
import com.music.bitchord.data.lyrics.LyricsRepository
import com.music.bitchord.data.lyrics.LyricsSource
import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpServer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import java.net.InetSocketAddress
import java.net.URLDecoder
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.security.MessageDigest
import java.time.Duration
import kotlin.system.exitProcess

private const val VERSION = "1.8.1-sidecar.1"
private const val QUERY_MAX = 200
private const val CONTINUATION_MAX = 4096
private val START = System.currentTimeMillis()

fun main(args: Array<String>) {
    val port = args
        .firstOrNull { it.startsWith("--port") }?.substringAfter('=')?.toIntOrNull()
        ?: args.getOrNull(args.indexOf("--port") + 1)?.toIntOrNull()
        ?: System.getenv("BITCHORD_SIDECAR_PORT")?.toIntOrNull()
        ?: 8765

    // Per-launch shared secret from the Rust launcher: every request must
    // carry it in the X-Sidecar-Token header. Loopback bind alone only
    // kept other local processes off the API by obscurity of the port.
    // No token set (dev/standalone runs) keeps the server open — the
    // launcher always sets one.
    val requiredToken = System.getenv("TANSEN_SIDECAR_TOKEN")

    fun authorized(ex: HttpExchange): Boolean {
        val expected = requiredToken ?: return true
        val provided = ex.requestHeaders.getFirst("X-Sidecar-Token") ?: return false
        return MessageDigest.isEqual(
            expected.toByteArray(Charsets.UTF_8),
            provided.toByteArray(Charsets.UTF_8),
        )
    }

    val server = HttpServer.create(InetSocketAddress("127.0.0.1", port), 32)
    server.executor = java.util.concurrent.Executors.newFixedThreadPool(8)

    server.createContext("/health") { ex -> handle(ex) { if (authorized(ex)) healthJson() else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/search") { ex -> handle(ex) { if (authorized(ex)) searchJson(ex) else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/track") { ex -> handle(ex) { if (authorized(ex)) trackJson(ex) else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/album") { ex -> handle(ex) { if (authorized(ex)) browseJson(ex, "album") else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/playlist") { ex -> handle(ex) { if (authorized(ex)) browseJson(ex, "playlist") else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/artist") { ex -> handle(ex) { if (authorized(ex)) browseJson(ex, "artist") else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/suggest") { ex -> handle(ex) { if (authorized(ex)) suggestJson(ex) else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/radio") { ex -> handle(ex) { if (authorized(ex)) radioEndpoint(ex) else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/home") { ex -> handle(ex) { if (authorized(ex)) homeEndpoint(ex) else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/explore") { ex -> handle(ex) { if (authorized(ex)) exploreEndpoint(ex) else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/browse/continuation") { ex -> handle(ex) { if (authorized(ex)) browseContinuationEndpoint(ex) else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/lyrics") { ex -> handle(ex) { if (authorized(ex)) lyricsJson(ex) else errorJson(401, "missing or wrong sidecar token") } }
    server.createContext("/") { ex -> handle(ex) { errorJson(404, "no such route") } }

    Runtime.getRuntime().addShutdownHook(Thread {
        log("shutting down (SIGTERM or JVM exit)")
        server.stop(0)
    })

    server.start()
    log("sidecar $VERSION listening on 127.0.0.1:$port")
    // The HTTP server's threads are daemon-free; park the main thread forever.
    Thread.currentThread().join()
}

// Handlers

private fun healthJson(): JsonObject = buildJsonObject {
    put("ok", JsonPrimitive(true))
    put("version", JsonPrimitive(VERSION))
    put("uptimeMs", JsonPrimitive(System.currentTimeMillis() - START))
}

private suspend fun searchJson(ex: HttpExchange): JsonObject {
    val query = ex.queryParam("q")?.trim() ?: return errorJson(400, "missing ?q=")
    if (query.isEmpty()) return errorJson(400, "query is empty")
    if (query.length > QUERY_MAX) return errorJson(400, "query too long")
    val filter = parseFilter(ex.queryParam("filter")) ?: return errorJson(400, "invalid search filter")
    val continuation = ex.queryParam("continuation")
    if (continuation != null && !validContinuation(continuation)) return errorJson(400, "invalid continuation")
    val page = if (continuation == null) {
        YtMusicRepository.searchPage(query, filter)
    } else {
        YtMusicRepository.searchContinuation(continuation, filter)
    }.getOrElse { return errorJson(502, "upstream: ${it.message}") }
    return searchResultsJson(page.rows, page.continuation)
}

private fun parseFilter(raw: String?): SearchFilter? = when (raw?.lowercase() ?: "all") {
    "all" -> SearchFilter.ALL
    "songs" -> SearchFilter.SONGS
    "albums" -> SearchFilter.ALBUMS
    "artists" -> SearchFilter.ARTISTS
    "playlists" -> SearchFilter.PLAYLISTS
    "videos" -> SearchFilter.VIDEOS
    else -> null
}

private suspend fun trackJson(ex: HttpExchange): JsonObject {
    val videoId = ex.pathSegment(1) ?: return errorJson(400, "missing video id")
    if (!validId(videoId)) return errorJson(400, "invalid video id")
    if (ex.requestURI.path.endsWith("/stream")) {
        val prefer = ex.queryParam("prefer")
        val preferCodec = ex.queryParam("prefer_codec")
        return streamJson(videoId, prefer, preferCodec)
    }
    val song = YtMusicRepository.trackLinks(videoId).getOrElse { return errorJson(404, "no watch entry: ${it.message}") }
    return songJson(song)
}

/**
 * The stream endpoint. Without `prefer=aac` it answers with whatever
 * [StreamResolver.resolve] picked (any container); with `prefer=aac` it asks
 * [StreamResolver.resolveForDownload] with `requireM4a = true`, which
 * guarantees an MP4/AAC rendition the Rust side's decoder can read directly.
 * Progressive audio itags only — HLS/DASH manifests are a later step and are
 * recorded as a gap in the desktop feature matrix.
 */
private suspend fun streamJson(videoId: String, prefer: String?, preferCodec: String?): JsonObject {
    // Optional codec-preference hint (backward compatible — absent =
    // previous behavior). Only "aac"/"m4a" is honored; anything else falls
    // back to the existing prefer/prefer=aac logic.
    val effectivePrefer = if (preferCodec == "aac" || preferCodec == "m4a") "aac" else prefer
    val preferAac = effectivePrefer == "aac" || effectivePrefer == "m4a"
    return if (preferAac) {
        val stream = StreamResolver.resolveForDownload(videoId, Int.MAX_VALUE, requireM4a = true)
        buildJsonObject {
            put("url", JsonPrimitive(stream.url))
            put("format", JsonPrimitive(stream.mimeType))
            put("kbps", JsonPrimitive(stream.kbps))
            itagOf(stream.url)?.let { put("itag", JsonPrimitive(it)) }
            stream.loudnessDb?.let { put("loudnessDb", JsonPrimitive(it)) }
            put("container", JsonPrimitive("mp4"))
            put("expiresAt", JsonPrimitive(System.currentTimeMillis() + STREAM_TTL_MS))
        }
    } else {
        val url = StreamResolver.resolve(videoId)
        buildJsonObject {
            put("url", JsonPrimitive(url))
            sniffedFormat(url)?.let { (mime, container) ->
                put("format", JsonPrimitive(mime))
                put("container", JsonPrimitive(container))
            }
            itagOf(url)?.let { put("itag", JsonPrimitive(it)) }
            StreamResolver.loudnessDbFor(videoId)?.let { put("loudnessDb", JsonPrimitive(it)) }
            put("expiresAt", JsonPrimitive(System.currentTimeMillis() + STREAM_TTL_MS))
        }
    }
}

private suspend fun browseJson(ex: HttpExchange, kind: String): JsonObject {
    val browseId = ex.pathSegment(1) ?: return errorJson(400, "missing browse id")
    val id = if (kind == "playlist" && !browseId.startsWith("VL")) "VL$browseId" else browseId
    if (!validId(id)) return errorJson(400, "invalid browse id")
    val page = YtMusicRepository.browseSongs(id).getOrElse { return errorJson(502, "upstream: ${it.message}") }
    return browsePageJson(page)
}

private suspend fun browseContinuationEndpoint(ex: HttpExchange): JsonObject {
    val token = ex.queryParam("token") ?: return errorJson(400, "missing continuation token")
    if (!validContinuation(token)) return errorJson(400, "invalid continuation")
    val page = YtMusicRepository.moreSongs(token).getOrElse { return errorJson(502, "upstream: ${it.message}") }
    return browsePageJson(page)
}

/**
 * Typeahead rows for discovery. Deliberately unauthenticated upstream
 * ([YtMusicRepository.searchTypeahead] strips the session cookie), so typed
 * keystrokes never land in the account's server-side search history. No
 * account sign-in, cookies or tokens cross this endpoint.
 */
private suspend fun suggestJson(ex: HttpExchange): JsonObject {
    val query = ex.queryParam("q")?.trim() ?: return errorJson(400, "missing ?q=")
    if (query.isEmpty()) return errorJson(400, "query is empty")
    if (query.length > QUERY_MAX) return errorJson(400, "query too long")
    val queries = YtMusicRepository.searchSuggestions(query).getOrElse { return errorJson(502, "upstream: ${it.message}") }
    val page = YtMusicRepository.searchTypeahead(query).getOrElse { return errorJson(502, "upstream: ${it.message}") }
    return searchResultsJson(page.rows, page.continuation, queries)
}

private suspend fun lyricsJson(ex: HttpExchange): JsonObject {
    val videoId = ex.pathSegment(2) ?: return errorJson(400, "missing video id")
    // Lyrics matching wants the recording's name, artist and runtime; the
    // watch-queue row carries all three.
    val song = YtMusicRepository.trackLinks(videoId).getOrElse { return errorJson(404, "no watch entry: ${it.message}") }
    val result = LyricsRepository.lyrics(
        videoId = videoId,
        title = song.title,
        artist = song.artist,
        durationMs = durationSeconds(song.durationText)?.times(1000) ?: 0L,
        sources = LyricsSource.entries.toSet(),
        order = LyricsSource.entries,
    ) ?: return errorJson(404, "no lyrics found")
    return lyricsJson(result.source.name, result.lines)
}

/**
 * Related tracks for autoplay — `:shared`'s radio (the watch queue
 * behind the playing track) as plain rows. Read-only; no account state.
 */
private suspend fun radioEndpoint(ex: HttpExchange): JsonObject {
    val videoId = ex.pathSegment(1) ?: return errorJson(400, "missing video id")
    if (!validId(videoId)) return errorJson(400, "invalid video id")
    val songs = YtMusicRepository.radio(videoId).getOrElse { return errorJson(502, "upstream: ${it.message}") }
    return radioJson(songs)
}

private suspend fun homeEndpoint(ex: HttpExchange): JsonObject {
    val token = ex.queryParam("continuation")
    if (token != null && !validContinuation(token)) return errorJson(400, "invalid continuation")
    val feed = if (token == null) YtMusicRepository.home() else YtMusicRepository.moreHome(token)
    return homeJson(feed.getOrElse { return errorJson(502, "upstream: ${it.message}") })
}

private suspend fun exploreEndpoint(ex: HttpExchange): JsonObject {
    val section = ex.queryParam("section") ?: return errorJson(400, "missing section")
    val browseId = when (section.lowercase()) {
        "explore" -> "FEmusic_explore"
        "new-releases" -> "FEmusic_new_releases"
        else -> return errorJson(400, "invalid explore section")
    }
    val shelves = YtMusicRepository.homeSupplement(browseId)
        .getOrElse { return errorJson(502, "upstream: ${it.message}") }
    return homeShelvesJson(shelves)
}

// Stream helper: mime sniff + itag extraction (adapter-level seam)

private const val STREAM_TTL_MS = 6 * 60 * 60_000L

private val http: HttpClient = HttpClient.newBuilder()
    .followRedirects(HttpClient.Redirect.NEVER)
    .connectTimeout(Duration.ofSeconds(10))
    .build()

/** The itag rides in every googlevideo URL's query. */
fun itagOf(url: String): Long? =
    url.split('?', '&').mapNotNull { it.split('=', limit = 2).takeIf { p -> p.size == 2 } }
        .firstOrNull { it[0] == "itag" }?.get(1)?.toLongOrNull()

/**
 * What the bytes at [url] actually are: one zero-byte ranged GET, read the
 * Content-Type. googlevideo answers ranged requests at line rate (see
 * ChunkedDataSource.kt for why), so this costs one round trip.
 */
fun sniffedFormat(url: String): Pair<String, String>? {
    return try {
        val response = http.send(
            HttpRequest.newBuilder(java.net.URI.create(url))
                .header("Range", "bytes=0-0")
                .GET().build(),
            HttpResponse.BodyHandlers.discarding(),
        )
        val mime = response.headers().firstValue("Content-Type").orElse(null) ?: return null
        val container = when {
            mime.contains("mp4", ignoreCase = true) -> "mp4"
            mime.contains("webm", ignoreCase = true) -> "webm"
            else -> mime.substringAfter('/')
        }
        mime to container
    } catch (e: Exception) {
        log("mime sniff failed: ${e.message}")
        null
    }
}

// HTTP plumbing

private fun HttpExchange.queryParam(name: String): String? =
    requestURI.rawQuery?.split('&')
        ?.mapNotNull { it.split('=', limit = 2).takeIf { p -> p.size == 2 } }
        ?.firstOrNull { it[0] == name }?.get(1)?.let { URLDecoder.decode(it, Charsets.UTF_8) }

private fun HttpExchange.pathSegment(index: Int): String? =
    requestURI.path.split('/').filter { it.isNotBlank() }.getOrNull(index)

private fun validContinuation(token: String): Boolean =
    token.isNotEmpty() && token.length <= CONTINUATION_MAX && token.none { it.isISOControl() }

private fun validId(id: String): Boolean =
    id.isNotEmpty() && id.length <= 64 && id.all { it.isLetterOrDigit() || it == '-' || it == '_' }

private fun errorJson(status: Int, message: String?): JsonObject = buildJsonObject {
    put("error", JsonPrimitive(message ?: "error"))
    // Carried so handle() can answer with the caller's exact status — the
    // message-prefix heuristic (missing→400, no→404, else 502) cannot
    // express 401/403/etc.
    put("status", JsonPrimitive(status))
}

private fun handle(ex: HttpExchange, body: suspend () -> JsonObject) {
    val started = System.currentTimeMillis()
    // Error-shaped bodies carry their status in-band: the caller's
    // explicit `status` field wins; the message-prefix heuristic
    // (missing→400, no→404, else 502) stays as the fallback for callers
    // that only put an `error` string.
    val (status, json) = try {
        val json = runBlocking { withContext(Dispatchers.IO) { body() } }
        if (json.containsKey("error")) {
            // Bind the status first: `a ?: b to json` types as the Int/Pair
            // intersection (reported as "Serializable"), which cannot
            // destructure. Both branches below are Pair<Int, JsonObject>.
            val status = explicitStatus(json) ?: statusForError(json)
            status to json
        } else {
            200 to json
        }
    } catch (e: Exception) {
        log("FAIL ${ex.requestMethod} ${ex.requestURI.path}: ${e.javaClass.simpleName}: ${e.message}")
        500 to errorJson(500, "${e.javaClass.simpleName}: ${e.message}")
    }
    send(ex, status, json)
    log("${if (status < 400) "OK" else "ERR"} $status ${ex.requestMethod} ${ex.requestURI.path} in ${System.currentTimeMillis() - started}ms")
}

private fun explicitStatus(json: JsonObject): Int? =
    (json["status"] as? JsonPrimitive)?.content?.toIntOrNull()

private fun statusForError(json: JsonObject): Int {
    val message = json["error"]?.toString().orEmpty()
    return when {
        message.startsWith("missing", ignoreCase = true) -> 400
        message.startsWith("no ", ignoreCase = true) -> 404
        else -> 502
    }
}

private fun send(ex: HttpExchange, status: Int, json: JsonObject) {
    val bytes = json.toString().toByteArray(Charsets.UTF_8)
    ex.responseHeaders.add("Content-Type", "application/json")
    ex.sendResponseHeaders(status, bytes.size.toLong().let { if (it == 0L) -1L else it })
    if (bytes.isNotEmpty()) ex.responseBody.use { it.write(bytes) }
    ex.close()
}

private fun log(message: String) {
    println("[sidecar] $message")
}
