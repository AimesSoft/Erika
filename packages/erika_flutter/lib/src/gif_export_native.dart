import 'gif_export_native_stub.dart'
    if (dart.library.io) 'gif_export_native_io.dart' as implementation;

Future<Map<String, Object?>> exportGifNative(Map<String, Object?> options) =>
    implementation.exportGifNative(options);
