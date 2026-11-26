# JNA looks up its classes and the UniFFI structures by name from native code.
-dontwarn java.awt.*
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-keep class cc.off42.mobile.** { *; }
