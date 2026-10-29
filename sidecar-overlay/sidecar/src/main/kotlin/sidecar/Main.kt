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
import java.time.Duration
import kotlin.system.exitProcess

private const val VERSION = "1.8.1-sidecar.1"
private val START = System.currentTimeMillis()

fun main(args: Array<String>) {
    val port = args
        .firstOrNull { it.startsWith("--port") }?.substringAfter('=')?.toIntOrNull()
        ?: args.getOrNull(args.indexOf("--port") + 1)?.toIntOrNull()
        ?: System.getenv("BITCHORD_SIDECAR_PORT")?.toIntOrNull()
        ?: 8765

    val server = HttpServer.create(InetSocketAddress("127.0.0.1", port), 32)
    server.executor = java.util.concurrent.Executors.newFixedThreadPool(8)

    server.createContext("/health") { ex -> handle(ex) { healthJson() } }
    server.createContext("/search") { ex -> handle(ex) { searchJson(ex) } }
    server.createContext("/track") { ex -> handle(ex) { trackJson(ex) } }
    server.createContext("/album") { ex -> handle(ex) { browseJson(ex, "album") } }
    server.createContext("/playlist") { ex -> handle(ex) { browseJson(ex, "playlist") } }
    server.createContext("/lyrics") { ex -> handle(ex) { lyricsJson(ex) } }
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

// ---- Handlers ---------------------------------------------------------------

private fun healthJson(): JsonObject = buildJsonObject {
    put("ok", JsonPrimitive(true))
    put("version", JsonPrimitive(VERSION))
    put("uptimeMs", JsonPrimitive(System.currentTimeMillis() - START))
}

private suspend fun searchJson(ex: HttpExchange): JsonObject {
    val query = ex.queryParam("q") ?: return errorJson(400, "missing ?q=")
    val filter = when (ex.queryParam("filter")?.lowercase()) {
        "songs" -> SearchFilter.SONGS
        "albums" -> SearchFilter.ALBUMS
        "artists" -> SearchFilter.ARTISTS
        "playlists" -> SearchFilter.PLAYLISTS
        "videos" -> SearchFilter.VIDEOS
        else -> SearchFilter.ALL
    }
    val results = YtMusicRepository.search(query, filter).getOrElse { return errorJson(502, "upstream: ${it.message}") }
    return searchResultsJson(results)
}

private suspend fun trackJson(ex: HttpExchange): JsonObject {
    val videoId = ex.pathSegment(1) ?: return errorJson(400, "missing video id")
    if (ex.requestURI.path.endsWith("/stream")) return streamJson(videoId, ex.queryParam("prefer"))
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
private suspend fun streamJson(videoId: String, prefer: String?): JsonObject {
    val preferAac = prefer == "aac" || prefer == "m4a"
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
    val page = YtMusicRepository.browseSongs(id).getOrElse { return errorJson(502, "upstream: ${it.message}") }
    return browsePageJson(page)
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

// ---- Stream helper: mime sniff + itag extraction (adapter-level seam) -------

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

// ---- HTTP plumbing ----------------------------------------------------------

private fun HttpExchange.queryParam(name: String): String? =
    requestURI.rawQuery?.split('&')
        ?.mapNotNull { it.split('=', limit = 2).takeIf { p -> p.size == 2 } }
        ?.firstOrNull { it[0] == name }?.get(1)?.let { URLDecoder.decode(it, Charsets.UTF_8) }

private fun HttpExchange.pathSegment(index: Int): String? =
    requestURI.path.split('/').filter { it.isNotBlank() }.getOrNull(index)

private fun errorJson(status: Int, message: String?): JsonObject = buildJsonObject {
    put("error", JsonPrimitive(message ?: "error"))
}

private fun handle(ex: HttpExchange, body: suspend () -> JsonObject) {
    val started = System.currentTimeMillis()
    // Error-shaped bodies carry their status in-band: an `error` object is
    // either a 400 (bad input), a 404 (no such track/lyrics) or a 502
    // (upstream failure), decided by the caller putting the right message.
    val (status, json) = try {
        val json = runBlocking { withContext(Dispatchers.IO) { body() } }
        if (json.containsKey("error")) {
            statusForError(json) to json
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
