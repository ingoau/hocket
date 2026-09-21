plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.serialization)
}

android {
    namespace = "app.hocket.core"
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
    // The Rust core (.so per ABI) lands here from scripts/build-android-core.sh. The directory may be
    // absent; NativeCore.isAvailable() reports that and the app falls back to FakeCore.
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        // The UniFFI-generated glue is unsigned-type heavy and opts in to experimental APIs itself.
        freeCompilerArgs.addAll("-Xexpect-actual-classes", "-opt-in=kotlin.ExperimentalUnsignedTypes")
    }
}

dependencies {
    api(libs.kotlinx.serialization.json)
    api(libs.kotlinx.coroutines.android)
    implementation("${libs.jna.get()}@aar")
    implementation(libs.androidx.annotation)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.turbine)
}
