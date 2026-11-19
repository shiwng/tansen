/*
 * Tansen sidecar — JSON mappings from the shared module's models to the wire
 * shapes the Rust desktop app reads. Field names here are the sidecar
 * contract; the Rust side mirrors them in crates/sources/src/yt (see
 * docs/sidecar-api.md).
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

package sidecar

import com.music.bitchord.data.model.SearchResult
import com.music.bitchord.data.YtMusicRepository
import com.music.bitchord.data.model.ArtistRef
import com.music.bitchord.data.model.HomeFeed
import com.music.bitchord.data.model.HomeShelf
import com.music.bitchord.data.model.Song
import com.music.bitchord.data.lyrics.LyricLine
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/** Duration text ("3:45") to seconds; null when absent or unparsable. */
fun durationSeconds(text: String?): Long? {
    if (text.isNullOrBlank()) return null
    val parts = text.split(':').map { it.trim().toLongOrNull() ?: return null }
    if (parts.isEmpty() || parts.any { it < 0 }) return null
    return parts.fold(0L) { acc, part -> acc * 60 + part }
}

fun artistJson(ref: ArtistRef): JsonObject = buildJsonObject {
    put("name", JsonPrimitive(ref.name))
    ref.browseId?.let { put("browseId", JsonPrimitive(it)) }
}

fun songJson(song: Song): JsonObject = buildJsonObject {
    put("videoId", JsonPrimitive(song.videoId))
    put("title", JsonPrimitive(song.title))
    put("artist", JsonPrimitive(song.artist))
    put("artists", JsonArray(song.artists.map { artistJson(it) }))
    song.albumName?.let { put("album", JsonPrimitive(it)) }
    song.albumId?.let { put("albumId", JsonPrimitive(it)) }
    put("durationText", song.durationText?.let { JsonPrimitive(it) } ?: JsonNull)
    put("durationSec", durationSeconds(song.durationText)?.let { JsonPrimitive(it) } ?: JsonNull)
    song.thumbnailUrl?.let { put("thumbnailUrl", JsonPrimitive(it)) }
    put("isVideo", JsonPrimitive(song.isVideo))
    song.isExplicit?.let { put("isExplicit", JsonPrimitive(it)) }
}

/** Search rows: tracks (with their promotion marked) and browse cards. */
fun searchResultsJson(rows: List<SearchResult>, continuation: String? = null, queries: List<String> = emptyList()): JsonObject = buildJsonObject {
    put(
        "tracks",
        JsonArray(
            rows.mapNotNull { row ->
                when (row) {
                    is SearchResult.TopTrack -> songJson(row.song) + ("top" to JsonPrimitive(true))
                    is SearchResult.Track -> songJson(row.song)
                    is SearchResult.Browse -> null
                }
            },
        ),
    )
    put(
        "browse",
        JsonArray(
            rows.mapNotNull { row ->
                (row as? SearchResult.Browse)?.item?.let { item ->
                    buildJsonObject {
                        put("kind", JsonPrimitive(item.type.name.lowercase()))
                        put("browseId", JsonPrimitive(item.browseId))
                        put("title", JsonPrimitive(item.title))
                        put("subtitle", JsonPrimitive(item.subtitle))
                        item.thumbnailUrl?.let { put("thumbnailUrl", JsonPrimitive(it)) }
                    }
                }
            },
        ),
    )
    put("continuation", continuation?.let { JsonPrimitive(it) } ?: JsonNull)
    put("queries", JsonArray(queries.map { JsonPrimitive(it) }))
}

/** A radio (related tracks) answer — the watch queue as plain rows. */
fun radioJson(songs: List<com.music.bitchord.data.model.Song>): JsonObject = buildJsonObject {
    put("tracks", JsonArray(songs.map { songJson(it) }))
}

/** One browse page (album, playlist or artist): header if the response carried one. */
fun browsePageJson(page: YtMusicRepository.SongPage): JsonObject = buildJsonObject {
    page.header?.let { header ->
        put("title", JsonPrimitive(header.title))
        put("subtitle", JsonPrimitive(header.subtitle))
        header.thumbnailUrl?.let { put("thumbnailUrl", JsonPrimitive(it)) }
    }
    page.description?.let { put("description", JsonPrimitive(it)) }
    put("continuation", page.continuation?.let { JsonPrimitive(it) } ?: JsonNull)
    put("tracks", JsonArray(page.songs.map { songJson(it) }))
    put("suggested", JsonArray(page.suggested.map { songJson(it) }))
    page.library?.let { library ->
        put("library", buildJsonObject {
            put("playlistId", JsonPrimitive(library.playlistId))
            put("saved", JsonPrimitive(library.saved))
        })
    }
}

/** Anonymous home/explore shelves. Account-only fields are deliberately absent. */
fun homeJson(feed: HomeFeed): JsonObject = buildJsonObject {
    put("continuation", feed.continuation?.let { JsonPrimitive(it) } ?: JsonNull)
    put("shelves", JsonArray(feed.shelves.map(::homeShelfJson)))
}

fun homeShelvesJson(shelves: List<HomeShelf>): JsonObject = buildJsonObject {
    put("continuation", JsonNull)
    put("shelves", JsonArray(shelves.map(::homeShelfJson)))
}

private fun homeShelfJson(shelf: HomeShelf): JsonObject = buildJsonObject {
    put("title", JsonPrimitive(shelf.title))
    put("subtitle", JsonPrimitive(shelf.subtitle))
    put("layout", JsonPrimitive(shelf.layout.name.lowercase()))
    shelf.moreBrowseId?.let { put("moreBrowseId", JsonPrimitive(it)) }
    shelf.moreParams?.let { put("moreParams", JsonPrimitive(it)) }
    put("items", JsonArray(shelf.items.map { item ->
        buildJsonObject {
            put("title", JsonPrimitive(item.title))
            put("subtitle", JsonPrimitive(item.subtitle))
            item.thumbnailUrl?.let { put("thumbnailUrl", JsonPrimitive(it)) }
            item.videoId?.let { put("videoId", JsonPrimitive(it)) }
            item.browseId?.let { put("browseId", JsonPrimitive(it)) }
            put("isVideo", JsonPrimitive(item.isVideo))
        }
    }))
}

/** Lyric lines: the timing rides with each line; the flag just summarizes. */
fun lyricsJson(source: String, lines: List<LyricLine>): JsonObject = buildJsonObject {
    put("source", JsonPrimitive(source))
    put("synced", JsonPrimitive(lines.any { it.timeMs > 0 }))
    put("lines", JsonArray(lines.map { line ->
        buildJsonObject {
            put("timeMs", JsonPrimitive(line.timeMs))
            put("text", JsonPrimitive(line.text))
        }
    }))
}

private operator fun JsonObject.plus(pair: Pair<String, JsonElement>): JsonObject =
    JsonObject(toMap() + mapOf(pair.first to pair.second))
