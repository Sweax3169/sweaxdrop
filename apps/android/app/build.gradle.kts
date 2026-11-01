plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.sweaxstudios.sweaxdrop"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.sweaxstudios.sweaxdrop"
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "1.0.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    signingConfigs {
        create("release") {
            // CI and local release builds pass a keystore through the
            // environment; without one the debug key signs (sideload only).
            val ks = System.getenv("SWEAXDROP_KEYSTORE")
            if (ks != null && file(ks).exists()) {
                storeFile = file(ks)
                storePassword = System.getenv("SWEAXDROP_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("SWEAXDROP_KEY_ALIAS") ?: "sweaxdrop"
                keyPassword = System.getenv("SWEAXDROP_KEY_PASSWORD") ?: System.getenv("SWEAXDROP_KEYSTORE_PASSWORD")
            } else {
                val dbg = signingConfigs.getByName("debug")
                storeFile = dbg.storeFile
                storePassword = dbg.storePassword
                keyAlias = dbg.keyAlias
                keyPassword = dbg.keyPassword
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("release")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    packaging {
        jniLibs.useLegacyPackaging = false
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

// The shared web UI (ui/dist) ships inside the APK as assets/ui.
val copyUi by tasks.registering(Sync::class) {
    from(rootProject.file("../../ui/dist"))
    into(layout.projectDirectory.dir("src/main/assets/ui"))
}
tasks.named("preBuild") { dependsOn(copyUi) }
