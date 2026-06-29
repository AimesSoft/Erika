Pod::Spec.new do |s|
  s.name             = 'erika_flutter'
  s.version          = '0.1.0'
  s.summary          = 'Flutter embedder glue for the Erika Rust media engine.'
  s.description      = <<-DESC
Flutter macOS plugin that hosts a CAMetalLayer and drives Erika through its C ABI.
                       DESC
  s.homepage         = 'https://github.com/AimesSoft/Erika'
  s.license          = { :type => 'MPL-2.0' }
  s.author           = { 'AimesSoft' => 'dev@aimesoft.com' }
  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'
  s.dependency 'FlutterMacOS'
  s.platform = :osx, '10.14'
  s.swift_version = '5.0'
  s.pod_target_xcconfig = {
    'OTHER_LDFLAGS' => '$(inherited) -framework QuartzCore -framework Metal'
  }
end
