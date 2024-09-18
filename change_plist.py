import plistlib

filename = './target/release/bundle/osx/Chop.app/Contents/Info.plist'

with open(filename, 'rb') as f:
    plist = plistlib.load(f)

doc_types = [
        {
            "CFBundleTypeName": "All",
            "CFBundleTypeRole": "Editor",
            "CFBundleTypeExtensions": ["*"],
            "LSHandlerRank": "Default",
            "LSItemContentTypes": [
                "public.text",
                "public.data",
                "public.source-code"
            ],
            "NSDocumentClass": "my_app.Document"
            }
        ]

plist['CFBundleDocumentTypes'] = doc_types

with open(filename, 'wb') as f:
    plistlib.dump(plist, f)
