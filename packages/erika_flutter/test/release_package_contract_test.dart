import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('pub package contains its release-facing files', () {
    for (final path in <String>[
      'LICENSE',
      'CHANGELOG.md',
      'README.md',
      'example/pubspec.yaml',
      'example/lib/main.dart',
      'native/include/erika.h',
      'native_artifacts.properties',
    ]) {
      expect(File(path).existsSync(), isTrue, reason: '$path is required');
    }
  });

  test('package and native artifact versions stay aligned', () {
    final pubspec = File('pubspec.yaml').readAsStringSync();
    final artifacts = File('native_artifacts.properties').readAsStringSync();
    final version = RegExp(r'^version:\s*(\S+)', multiLine: true)
        .firstMatch(pubspec)!
        .group(1)!;

    expect(artifacts, contains('ERIKA_NATIVE_VERSION=$version'));
    for (final path in <String>[
      'ios/erika_flutter.podspec',
      'macos/erika_flutter.podspec',
      'tvos/erika_flutter.podspec',
      'ohos/oh-package.json5',
    ]) {
      expect(File(path).readAsStringSync(), contains(version), reason: path);
    }
  });

  test('published documentation does not link outside the package', () {
    for (final path in <String>['README.md', 'README.zh.md', 'README.ja.md']) {
      final readme = File(path).readAsStringSync();
      expect(readme, isNot(contains('](../../')), reason: path);
    }
  });

  test('native consumers default to verified prebuilt artifacts', () {
    final files = <String>[
      'android/erika-native.gradle',
      'ios/erika_flutter.podspec',
      'macos/erika_flutter.podspec',
      'tvos/erika_flutter.podspec',
      'windows/build_erika_runtime.cmake',
      'ohos/src/main/cpp/CMakeLists.txt',
    ].map((path) => File(path).readAsStringSync()).join('\n');

    expect(files, contains('ERIKA_FORCE_SOURCE_BUILD'));
    expect(files, contains('ERIKA_PREBUILT_SHA256'));
    expect(files, contains('SHA-256'));
    expect(files, isNot(contains('falling back to a source build')));
  });

  test('Android downloads verified per-ABI Flutter runtimes', () {
    final android = File('android/erika-native.gradle').readAsStringSync();
    final artifacts = File('native_artifacts.properties').readAsStringSync();

    expect(android, contains('erika-flutter-android-\$abi'));
    expect(android, isNot(contains('erika-capi-android.zip')));
    for (final property in <String>[
      'ERIKA_ANDROID_ARM64_V8A_SHA256',
      'ERIKA_ANDROID_ARMEABI_V7A_SHA256',
      'ERIKA_ANDROID_X86_64_SHA256',
      'ERIKA_ANDROID_X86_SHA256',
    ]) {
      expect(artifacts, contains('$property='), reason: property);
    }
  });
}
