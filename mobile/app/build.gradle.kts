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
        // 28, não 35: no Android 10+ (SELinux, domínio untrusted_app), um app com
        // targetSdk >= 29 não pode executar com exec() nenhum arquivo dentro da
        // própria pasta de dados (Behavior changes: apps targeting Android 10,
        // "Exec restrictions"). O pipeline "Baixar e preparar recursos vanilla"
        // executa, via ProcessBuilder, o OpenJDK 25 ARM64 e o horizon-prepare
        // instalados em filesDir; com targetSdk 29+ ambos morrem com
        // "Permission denied" em qualquer aparelho sem root. Com targetSdk 28
        // (domínio untrusted_app_27) a execução é permitida, como no Termux.
        // O app só usa armazenamento privado, então nada do que ele precisa é
        // perdido com o targetSdk menor.
        targetSdk = 28
        versionCode = 2
        versionName = "0.1.1"

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
