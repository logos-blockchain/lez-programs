import QtQuick

WalletIconButton {
    id: root

    signal copyRequested

    property string copyText: ""
    property string copyLabel: qsTr("Copy")
    property bool copied: false

    accessibleName: root.copied ? qsTr("Copied") : root.copyLabel
    iconSource: root.copied
        ? Qt.resolvedUrl("icons/checkmark.svg")
        : Qt.resolvedUrl("icons/copy.svg")

    Timer {
        id: resetTimer
        interval: 1500
        onTriggered: root.copied = false
    }

    TextEdit {
        id: clipboardProxy

        visible: false
    }

    function copyToClipboard() {
        if (root.copyText.length === 0)
            return
        clipboardProxy.text = root.copyText
        clipboardProxy.selectAll()
        clipboardProxy.copy()
        clipboardProxy.deselect()
        clipboardProxy.text = ""
    }

    onClicked: {
        root.copyToClipboard()
        root.copyRequested()
        root.copied = true
        resetTimer.restart()
    }
}
