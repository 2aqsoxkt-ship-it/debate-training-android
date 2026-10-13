import com.android.build.gradle.LibraryExtension

buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:8.11.0")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:1.9.25")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

// Pin the verified installed tools for the Tauri Android library too, without
// editing downloaded dependency sources or triggering an unpinned SDK install.
subprojects {
    // Each ASCII staging project owns its outputs. Never build inside the
    // shared Cargo registry source, where ARM64/x86_64 can lock the same JAR.
    if (name == "tauri-android") {
        layout.buildDirectory.set(rootProject.layout.projectDirectory.dir("tauri-library-build"))
    }
    plugins.withId("com.android.library") {
        extensions.configure<LibraryExtension> {
            buildToolsVersion = "36.0.0"
        }
    }
}

tasks.register("clean").configure {
    delete("build")
}
