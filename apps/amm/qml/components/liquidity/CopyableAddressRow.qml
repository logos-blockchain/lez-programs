pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

RowLayout {
    id: root

    required property var theme
    property string label: ""
    property string address: ""
    property string fallbackText: qsTr("Assigned by wallet")
    readonly property bool canCopy: root.address.length > 0

    Layout.fillWidth: true
    spacing: 8
    implicitHeight: Math.max(labelText.implicitHeight,
                             Math.max(addressText.implicitHeight, copyAddressButton.implicitHeight))

    Text {
        id: labelText

        Layout.minimumWidth: 72
        Layout.preferredWidth: 112
        Layout.maximumWidth: 140
        color: root.theme.colors.textSecondary
        font.pixelSize: 12
        text: root.label
        wrapMode: Text.Wrap
    }

    Text {
        id: addressText

        objectName: "addressText"
        Layout.fillWidth: true
        Layout.minimumWidth: 0
        color: root.canCopy ? root.theme.colors.textPrimary : root.theme.colors.textSecondary
        elide: root.canCopy ? Text.ElideMiddle : Text.ElideRight
        font.family: "monospace"
        font.pixelSize: 12
        font.weight: Font.Medium
        horizontalAlignment: Text.AlignRight
        text: root.canCopy ? root.address : root.fallbackText
        verticalAlignment: Text.AlignVCenter
        wrapMode: Text.NoWrap
    }

    HoverHandler {
        id: addressHover

        enabled: root.canCopy
    }

    ToolTip.visible: root.canCopy && addressText.truncated && addressHover.hovered
    ToolTip.text: root.address

    AmmCopyButton {
        id: copyAddressButton

        objectName: "copyAddressButton"
        Layout.preferredWidth: visible ? 48 : 0
        Layout.preferredHeight: 24
        theme: root.theme
        value: root.address
        accessibleName: qsTr("Copy %1 address").arg(root.label)
        buttonWidth: 48
        labelFontPixelSize: 11
    }
}
