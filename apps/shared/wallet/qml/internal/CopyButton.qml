import QtQuick

WalletIconButton {
    id: root

    signal copyRequested

    property bool copied: false

    accessibleName: root.copied ? qsTr("Copied") : qsTr("Copy")
    iconSource: root.copied
        ? Qt.resolvedUrl("icons/checkmark.svg")
        : Qt.resolvedUrl("icons/copy.svg")

    Timer {
        id: resetTimer
        interval: 1500
        onTriggered: root.copied = false
    }

    onClicked: {
        root.copyRequested()
        root.copied = true
        resetTimer.restart()
    }
}
