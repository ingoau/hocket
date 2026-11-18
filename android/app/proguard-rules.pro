-keep class app.hocket.core.api.** { *; }
-keepattributes *Annotation*, InnerClasses
-dontwarn org.slf4j.**

# ML Kit (the setup-code scanner) builds its services from Firebase component registrars it
# instantiates by reflection from manifest meta-data. firebase-components' consumer rule keeps the
# classes but not their constructors, which R8 full mode then strips, leaving ML Kit's services
# unresolved: GmsBarcodeScanning.getClient threw an NPE.
-keep class * implements com.google.firebase.components.ComponentRegistrar { <init>(); }
