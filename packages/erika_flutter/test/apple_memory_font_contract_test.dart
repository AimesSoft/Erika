import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  for (final platform in <String>['ios', 'tvos']) {
    test('$platform exposes and force-links subtitle memory fonts', () {
      final plugin = File(
        '$platform/Classes/ErikaFlutterPlugin.swift',
      ).readAsStringSync();
      final podspec = File(
        '$platform/erika_flutter.podspec',
      ).readAsStringSync();

      for (final method in <String>[
        'registerSubtitleMemoryFont',
        'selectSubtitleMemoryFonts',
        'clearSubtitleMemoryFonts',
        'getSubtitleMemoryFontStatus',
      ]) {
        expect(plugin, contains('case "$method"'));
      }

      for (final symbol in <String>[
        'erika_presenter_register_subtitle_memory_font',
        'erika_presenter_select_subtitle_memory_fonts',
        'erika_presenter_clear_subtitle_memory_fonts',
        'erika_presenter_get_subtitle_memory_font_status',
        'erika_subtitle_memory_font_status_free',
      ]) {
        expect(plugin, contains('"$symbol"'));
        expect(podspec, contains(symbol));
      }
    });
  }
}
