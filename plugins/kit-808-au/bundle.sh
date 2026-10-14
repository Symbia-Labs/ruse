#!/bin/zsh
# Build the MUSE 808 Audio Unit (a Music Device, aumu) and assemble its .component bundle
# in plugins/kit-808/dist. Installing it is a separate step:
#   ditto plugins/kit-808/dist/MUSE\ 808.component ~/Library/Audio/Plug-Ins/Components/MUSE\ 808.component
#   auval -v aumu K808 Symb
# package_plugin in muse-mcp packages effects only so far; this script stands in for it.
set -e
cd "$(dirname "$0")/../.."
cargo build --release -p plugin-kit-808-au
B="plugins/kit-808/dist/MUSE 808.component"
mkdir -p "$B/Contents/MacOS"
cp target/release/libkit_808_au.dylib "$B/Contents/MacOS/MUSE 808"
cat > "$B/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>English</string>
	<key>CFBundleExecutable</key>
	<string>MUSE 808</string>
	<key>CFBundleIdentifier</key>
	<string>symbia-labs.kit-808.au</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>MUSE 808</string>
	<key>CFBundlePackageType</key>
	<string>BNDL</string>
	<key>CFBundleSignature</key>
	<string>????</string>
	<key>CFBundleShortVersionString</key>
	<string>0.0.1</string>
	<key>CFBundleVersion</key>
	<string>0.0.1</string>
	<key>AudioComponents</key>
	<array>
		<dict>
			<key>name</key>
			<string>Symbia Labs: MUSE 808</string>
			<key>description</key>
			<string>An 808-style drum machine synthesized from oscillators, noise and filters.</string>
			<key>manufacturer</key>
			<string>Symb</string>
			<key>subtype</key>
			<string>K808</string>
			<key>type</key>
			<string>aumu</string>
			<key>version</key>
			<integer>1</integer>
			<key>factoryFunction</key>
			<string>Kit808Factory</string>
			<key>sandboxSafe</key>
			<true/>
			<key>tags</key>
			<array>
				<string>Synthesizer</string>
				<string>Drums</string>
			</array>
		</dict>
	</array>
</dict>
</plist>
PLIST
printf 'BNDL????' > "$B/Contents/PkgInfo"
codesign --force --sign - "$B"
echo "$B"
