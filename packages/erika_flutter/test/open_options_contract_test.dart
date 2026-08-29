import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('Windows accepts both StandardMessageCodec integer widths', () {
    final plugin = File(
      'windows/erika_flutter_plugin.cpp',
    ).readAsStringSync();
    final parseStart = plugin.indexOf(
      'if (const auto* raw_read_ahead = FindArg(args, "httpReadAheadBytes")',
    );
    final parseEnd = plugin.indexOf(
      'const bool wants_headers',
      parseStart,
    );

    expect(parseStart, greaterThanOrEqualTo(0));
    expect(parseEnd, greaterThan(parseStart));
    final parser = plugin.substring(parseStart, parseEnd);
    expect(parser, contains('std::get_if<int32_t>'));
    expect(parser, contains('std::get_if<int64_t>'));
    expect(parser, contains('*value < 0'));
  });

  for (final platform in <String>['ios', 'macos', 'tvos']) {
    test('$platform validates read-ahead before converting to UInt64', () {
      final plugin = File(
        '$platform/Classes/ErikaFlutterPlugin.swift',
      ).readAsStringSync();

      expect(
        plugin,
        contains(
          'let readAhead = try optionalReadAheadBytes('
          'args["httpReadAheadBytes"])',
        ),
      );
      expect(plugin, contains('numericValue >= 0'));
      expect(plugin,
          contains('numericValue.rounded(.towardZero) == numericValue'));
    });
  }
}
