python3 change_plist.py
rm -rf /Applications/Chop.app
mv ./target/release/bundle/osx/Chop.app /Applications
# codesign --force --sign \
# 	"Apple Development: Jason Joe Eveleth (73PMM4JFMA)" \
# 	./target/release/bundle/osx/Chop.app
