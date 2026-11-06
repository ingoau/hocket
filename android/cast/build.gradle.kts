// The Google Cast SDK renderer (CastDevices' provider). Proprietary (Google Play services): the app
// leaves this module out of a FOSS build (-Phocket.foss=true; android/README.md, Cast).
plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.android)
}

android {
    namespace = "app.hocket.cast"
    compileSdk = 37
    compileSdkMinor = 1

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    testOptions {
        unitTests.isReturnDefaultValues = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(project(":playback"))
    implementation(libs.play.services.cast.framework)
    implementation(libs.kotlinx.coroutines.android)

    testImplementation(libs.junit)
}
