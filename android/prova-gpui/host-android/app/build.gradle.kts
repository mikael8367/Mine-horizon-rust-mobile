plugins { id("com.android.application"); id("org.jetbrains.kotlin.android") }
val gpuiAndroidSrc = System.getenv("GPUI_ANDROID_SRC") ?: error("Use ../build-apk.sh para localizar o backend GPUI fixado no Cargo.lock")
android {
    namespace = "br.com.minehorizon.gpuiprova"
    compileSdk = 36
    ndkVersion = "27.1.12297006"
    defaultConfig {
        applicationId = "br.com.minehorizon.gpuiprova"
        minSdk = 30
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += "arm64-v8a" }
    }
    sourceSets["main"].java.srcDir(gpuiAndroidSrc)
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions { jvmTarget = "17" }
    packaging { jniLibs.useLegacyPackaging = false }
}
