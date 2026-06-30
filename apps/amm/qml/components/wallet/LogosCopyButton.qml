import QtQuick 2.15
import QtQuick.Controls 2.15

import Logos.Theme

Button {
    id: root

    signal copyText()

    property string accessibleName: ""
    property string iconSource: Qt.resolvedUrl("icons/copy.svg")
    property bool copied: false

    implicitWidth: 24
    implicitHeight: 24
    text: root.copied ? qsTr("Copied") : root.accessibleName
    Accessible.name: text
    display: AbstractButton.IconOnly
    flat: true

    icon.source: root.iconSource
    icon.width: 24
    icon.height: 24
    icon.color: Theme.palette.textSecondary

    function reset() {
        iconSource = Qt.resolvedUrl("icons/copy.svg")
        copied = false
    }

    Timer {
        id: resetTimer
        interval: 1500
        repeat: false
        onTriggered: root.reset()
    }

    onClicked: {
        root.copyText()
        root.iconSource = Qt.resolvedUrl("icons/checkmark.svg")
        root.copied = true
        resetTimer.restart()
    }
}
