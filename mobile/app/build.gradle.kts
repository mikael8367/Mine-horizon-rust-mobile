plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")

    namespace = "dev.playhorizon.mobile"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.playhorizon.mobile"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"

        // This port currently ships only the Rust engine built for Android ARM64.
        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    // Extract JNI libraries into nativeLibraryDir. NativeActivity loads
    // libhorizon_android.so through Android's native-library loader.
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}
