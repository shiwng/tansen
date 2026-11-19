/*
 * Tansen sidecar — a local HTTP server exposing the shared module's YouTube
 * Music client to the Rust desktop app.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 * Compiled against the pinned upstream sources; this module changes nothing
 * in :shared, it only calls it. See scripts/build-sidecar.sh.
 */

plugins {
    id("org.jetbrains.kotlin.jvm")
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

dependencies {
    implementation(project(":shared"))
    // NewPipe is compileOnly in :shared (the phone ships a trimmed jar);
    // the sidecar runs the real thing, so it supplies it here.
    // Pinned to the commit v0.26.3 pointed at; tags are mutable on JitPack.
    implementation("com.github.TeamNewPipe:NewPipeExtractor:caae86c943857cc6e1a762e3488d6a14e9cf7800")
}

/**
 * A self-contained jar: main class plus the whole runtime classpath
 * (:shared, Kotlin stdlib, coroutines, serialization, OkHttp, NewPipe,
 * InnerTubeX…). The Rust side launches `java -jar sidecar-all.jar`.
 */
val fatJar = tasks.register<Jar>("fatJar") {
    archiveBaseName.set("sidecar")
    archiveClassifier.set("all")
    duplicatesStrategy = DuplicatesStrategy.EXCLUDE
    manifest { attributes["Main-Class"] = "sidecar.MainKt" }
    from(sourceSets.main.get().output)
    dependsOn(configurations.runtimeClasspath)
    from({
        configurations.runtimeClasspath.get().map { if (it.isDirectory) it else zipTree(it) }
    }) {
        // Signed-jar leftovers break the merge; none of it is used on the JVM.
        exclude("META-INF/*.SF", "META-INF/*.DSA", "META-INF/*.RSA")
    }
}

tasks.build { dependsOn(fatJar) }

tasks.test {
    useJUnitPlatform()
}
