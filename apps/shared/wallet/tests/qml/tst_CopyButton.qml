import QtQuick
import QtTest

import Logos.Wallet as Wallet

Item {
    id: root

    width: 360
    height: 240

    Component {
        id: copyButtonComponent

        Wallet.CopyButton {}
    }

    Component {
        id: clipboardSinkComponent

        TextEdit {}
    }

    SignalSpy {
        id: copyRequestedSpy
    }

    TestCase {
        name: "CopyButton"
        when: windowShown

        function test_copiesTextAndRetainsCopySignal() {
            const value = "1thX6LZfHDZZKUs92febYZhYRcXddmzfzF2NvTkPNE"
            const copyButton = createTemporaryObject(copyButtonComponent, root, {
                "copyText": value,
                "copyLabel": "Copy address"
            })
            const sink = createTemporaryObject(clipboardSinkComponent, root)
            verify(copyButton, "Copy button exists")
            verify(sink, "Clipboard sink exists")
            compare(copyButton.implicitWidth, 36)
            compare(copyButton.implicitHeight, 36)

            copyRequestedSpy.target = copyButton
            copyRequestedSpy.signalName = "copyRequested"
            copyRequestedSpy.clear()
            copyButton.click()

            verify(copyButton.copied)
            compare(copyRequestedSpy.count, 1)
            sink.paste()
            tryCompare(sink, "text", value)
            copyRequestedSpy.target = null
        }
    }
}
