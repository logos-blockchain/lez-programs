import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

import Logos.Theme
import Logos.Controls

Popup {
    id: root

    property string mnemonic: ""
    readonly property real viewportMargin: Theme.spacing.large

    modal: true
    dim: true
    padding: Theme.spacing.large
    closePolicy: Popup.NoAutoClose
    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(460, Math.max(0, parent ? parent.width - root.viewportMargin * 2 : 460))

    onOpened: savedCheck.checked = false
    onClosed: root.mnemonic = ""

    background: Rectangle {
        color: Theme.palette.backgroundSecondary
        radius: Theme.spacing.radiusXlarge
        border.color: Theme.palette.backgroundElevated
    }

    contentItem: ColumnLayout {
        width: root.availableWidth
        spacing: Theme.spacing.large

        LogosText {
            text: qsTr("Back up your wallet")
            font.pixelSize: Theme.typography.titleText
            font.weight: Theme.typography.weightBold
            color: Theme.palette.text
        }

        LogosText {
            Layout.fillWidth: true
            text: qsTr("Save this recovery phrase now. You need it to restore this wallet.")
            font.pixelSize: Theme.typography.secondaryText
            color: Theme.palette.textSecondary
            wrapMode: Text.WordWrap
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: phraseText.implicitHeight + Theme.spacing.medium * 2
            radius: Theme.spacing.radiusLarge
            color: Theme.palette.backgroundMuted
            border.color: Theme.palette.backgroundElevated

            TextEdit {
                id: phraseText
                anchors.fill: parent
                anchors.margins: Theme.spacing.medium
                readOnly: true
                selectByMouse: true
                textFormat: TextEdit.PlainText
                wrapMode: Text.WordWrap
                text: root.mnemonic
                color: Theme.palette.text
                font.pixelSize: Theme.typography.secondaryText
                background: null
                Accessible.name: qsTr("Recovery phrase")
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacing.medium

            CheckBox {
                id: savedCheck
                Layout.fillWidth: true
                text: qsTr("I saved this recovery phrase")
                checked: false
            }
        }

        LogosButton {
            Layout.fillWidth: true
            height: 40
            text: qsTr("Continue")
            enabled: savedCheck.checked
            onClicked: root.close()
        }
    }
}
