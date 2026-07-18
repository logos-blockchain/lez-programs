pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic

Button {
    id: root

    required property var theme
    property string value: ""
    property string accessibleName: qsTr("Copy address")
    property int buttonWidth: 30
    property int labelFontPixelSize: 10
    property bool copied: false

    implicitWidth: root.buttonWidth
    implicitHeight: 24
    visible: root.value.length > 0
    enabled: root.value.length > 0
    hoverEnabled: true
    text: root.copied ? "\u2713" : qsTr("Copy")
    Accessible.name: root.copied ? qsTr("Copied") : root.accessibleName
    ToolTip.visible: hovered
    ToolTip.text: Accessible.name
    onClicked: {
        root.copyToClipboard()
        root.copied = true
        copiedReset.restart()
    }

    contentItem: Text {
        text: root.text
        color: root.enabled ? root.theme.colors.ctaBg : root.theme.colors.textPlaceholder
        font.pixelSize: root.labelFontPixelSize
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

    TextEdit {
        id: clipboardProxy

        visible: false
    }

    function copyToClipboard() {
        clipboardProxy.text = root.value
        clipboardProxy.selectAll()
        clipboardProxy.copy()
        clipboardProxy.deselect()
        clipboardProxy.text = ""
    }
}
