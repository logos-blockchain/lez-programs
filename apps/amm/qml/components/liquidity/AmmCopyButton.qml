pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic

Button {
    id: root

    required property var theme
    property string value: ""
    property bool copied: false

    signal copyRequested(string value)

    implicitWidth: 30
    implicitHeight: 24
    visible: root.value.length > 0
    enabled: root.value.length > 0
    hoverEnabled: true
    text: root.copied ? "\u2713" : qsTr("Copy")
    Accessible.name: root.copied ? qsTr("Copied") : qsTr("Copy address")
    ToolTip.visible: hovered
    ToolTip.text: Accessible.name
    onClicked: {
        root.copied = true
        copiedReset.restart()
        root.copyRequested(root.value)
    }

    contentItem: Text {
        text: root.text
        color: root.enabled ? root.theme.colors.ctaBg : root.theme.colors.textPlaceholder
        font.pixelSize: 10
        font.weight: Font.Medium
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    background: Rectangle {
        radius: 5
        color: root.hovered || root.activeFocus ? root.theme.colors.selection : "transparent"
    }

    Timer {
        id: copiedReset

        interval: 1600
        repeat: false
        onTriggered: root.copied = false
    }
}
