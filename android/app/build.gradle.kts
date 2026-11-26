plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// The Rust engine (crate rust/crates/off42-mobile) is built by scripts/build-rust.sh into rust-out/:
//   rust-out/jniLibs/<abi>/liboff42_mobile.so   the app's native library
//   rust-out/kotlin/                            UniFFI Kotlin bindings (package cc.off42.mobile)
//   rust-out/host/                              a library for the JVM unit tests on the build machine
val rustOut = layout.projectDirectory.dir("rust-out")
val rustBindings = rustOut.dir("kotlin").dir("cc/off42/mobile").file("off42_mobile.kt").asFile
if (!rustBindings.exists()) {
    throw GradleException("Missing ${rustBindings.path}: run android/scripts/build-rust.sh first (see android/README.md).")
}

// Release signing comes from the environment (CI secrets); without it the release bundle is left unsigned.
val keystoreFile = System.getenv("ANDROID_KEYSTORE_FILE")?.takeIf { it.isNotBlank() && file(it).exists() }

android {
    namespace = "cc.off42.office"
    compileSdk = 37
    compileSdkMinor = 2

    defaultConfig {
        applicationId = "cc.off42.office"
        minSdk = 26
        targetSdk = 36
        versionCode = (System.getenv("OFF42_VERSION_CODE") ?: "1").toInt()
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        testInstrumentationRunnerArguments["useTestStorageService"] = "true"
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    signingConfigs {
        if (keystoreFile != null) {
            create("release") {
                storeFile = file(keystoreFile)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            if (keystoreFile != null) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    sourceSets {
        getByName("main") {
            kotlin.directories += rustOut.dir("kotlin").asFile.path
            jniLibs.directories += rustOut.dir("jniLibs").asFile.path
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
    }

    lint {
        abortOnError = true
        checkDependencies = false
        // Version-freshness checks need the network and change from day to day; they are reviewed by hand.
        disable += listOf("GradleDependency", "NewerVersionAvailable", "AndroidGradlePluginVersion", "OldTargetApi")
        // UniFFI's generated bindings are not ours to lint.
        ignoreTestSources = true
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
    }

    packaging {
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
}

kotlin {
    jvmToolchain(17)
}

tasks.withType<Test>().configureEach {
    // The JVM unit tests call the engine through JNA with a library built for the build machine.
    systemProperty("jna.library.path", rustOut.dir("host").asFile.path)
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material3.window.size)
    implementation(libs.compose.material.icons)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.jna.aar) { artifact { type = "aar" } }
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(libs.junit)
    testImplementation(libs.jna.jar)
    testImplementation(libs.kotlinx.coroutines.test)

    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.test.core)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.rules)
    androidTestImplementation(libs.androidx.test.junit)
    androidTestImplementation(libs.androidx.test.uiautomator)
    androidTestImplementation(libs.androidx.test.espresso.intents)
    androidTestUtil(libs.androidx.test.services)
}
