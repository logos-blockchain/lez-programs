import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

import Logos.Theme
import Logos.Controls

// One account row in the account dropdown. Ported from the LEZ wallet UI.
ItemDelegate {
    id: root

    // Emitted when the user clicks the copy icon; the parent view connects this
    // to its QML-side clipboard helper (AccountControl.copyToClipboard).
    signal copyRequested(string text)

    property bool selectable: model.isPublic ?? false
    readonly property string displayAddress: model.displayAddress ?? ""

    focusPolicy: root.selectable ? Qt.StrongFocus : Qt.NoFocus
    activeFocusOnTab: root.selectable
    Accessible.role: root.selectable ? Accessible.RadioButton : Accessible.ListItem
    Accessible.name: root.selectable
                     ? qsTr("Select account %1").arg(root.displayAddress)
                     : qsTr("Private account %1").arg(root.displayAddress)
    Accessible.description: root.selectable ? "" : qsTr("Private accounts cannot be used for AMM actions")
    Accessible.checked: root.highlighted

    leftPadding: Theme.spacing.medium
    rightPadding: Theme.spacing.medium
    topPadding: Theme.spacing.medium
    bottomPadding: Theme.spacing.medium

    background: Rectangle {
        color: root.highlighted || root.hovered ?
                   Theme.palette.backgroundMuted :
                   Theme.palette.backgroundTertiary
        radius: Theme.spacing.radiusLarge
    }

    contentItem: ColumnLayout {
        spacing: Theme.spacing.small
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacing.small

            LogosText {
                text: model.name ?? ""
                font.pixelSize: Theme.typography.secondaryText
                font.bold: true
            }

            Rectangle {
                Layout.preferredWidth: tagLabel.implicitWidth + Theme.spacing.small * 2
                Layout.preferredHeight: tagLabel.implicitHeight + 4
                radius: 4
                color: Theme.palette.backgroundSecondary

                LogosText {
                    id: tagLabel
                    anchors.centerIn: parent
                    text: model.isPublic ? qsTr("Public") : qsTr("Private")
                    font.pixelSize: Theme.typography.secondaryText
                    color: Theme.palette.textSecondary
                }
            }

            Item { Layout.fillWidth: true }

            LogosText {
                text: model.balance && model.balance.length > 0 ? model.balance : "—"
                font.bold: true
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 0
            LogosText {
                id: addressLabel
                Layout.fillWidth: true
                verticalAlignment: Text.AlignVCenter
                text: root.displayAddress
                font.pixelSize: Theme.typography.secondaryText
                color: Theme.palette.textMuted
                elide: Text.ElideMiddle
            }
            LogosCopyButton {
                Layout.preferredHeight: 40
                Layout.preferredWidth: 40
                accessibleName: qsTr("Copy account address")
                onCopyText: root.copyRequested(root.displayAddress)
                visible: root.displayAddress.length > 0
                icon.color: Theme.palette.textMuted
            }
        }
    }
}
