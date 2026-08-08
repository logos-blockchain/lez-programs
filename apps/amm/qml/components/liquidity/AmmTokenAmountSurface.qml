pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root

    required property var theme
    property string label: ""
    property string amount: ""
    property string supportingText: ""
    property string errorText: ""
    property string supportingActionText: ""
    property bool readOnly: false
    property bool muted: false
    property bool invalid: root.errorText.length > 0
    property Component accessory
    property real accessoryWidth: 0
    property real accessoryHeight: 0
    property Component adjustment
    property real adjustmentWidth: 0
    property real adjustmentHeight: 0
    // Optional full-width content rendered inside the card, below the amount/token
    // row (e.g. the create-pool account selector). Null → the card is unchanged.
    property Component footer
    property real footerHeight: 0
    readonly property bool footerActive: root.footer !== null
    property alias footerItem: footerLoader.item

    signal amountEdited(string value)
    signal amountEditingFinished(string value)
    signal supportingActionClicked

    implicitHeight: root.footerActive
                    ? Math.max(110, contentRow.implicitHeight + root.footerHeight + 30)
                    : Math.max(110, contentRow.implicitHeight + 24)
    radius: 16
    color: root.muted ? root.theme.colors.panelBg : root.theme.colors.inputBg
    border.color: root.invalid
                  ? root.theme.colors.error
                  : amountInput.activeFocus
                    ? root.theme.colors.ctaBg
                    : "transparent"
    border.width: 1

    Binding {
        target: amountInput
        property: "text"
        value: root.amount
    }

    RowLayout {
        id: contentRow

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        // When a footer is present it takes the bottom of the card; otherwise the
        // row fills the card exactly as before (12px inset).
        anchors.bottom: footerLoader.top
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        anchors.topMargin: 12
        anchors.bottomMargin: root.footerActive ? 6 : 12
        spacing: 10

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2

            Text {
                text: root.label
                color: root.theme.colors.textSecondary
                font.pixelSize: 13
                elide: Text.ElideRight
                Layout.fillWidth: true
            }

            Text {
                Layout.fillWidth: true
                visible: root.errorText.length > 0
                text: root.errorText
                color: root.theme.colors.error
                font.pixelSize: 11
                wrapMode: Text.Wrap
            }

            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: 44

                TextInput {
                    id: amountInput

                    anchors.fill: parent
                    readOnly: root.readOnly
                    color: root.readOnly || root.muted
                           ? root.theme.colors.textSecondary
                           : root.theme.colors.textPrimary
                    font.pixelSize: {
                        var length = Math.max(1, String(root.amount || "0").length)
                        return Math.max(14, Math.min(34, Math.floor(width / (length * 0.68))))
                    }
                    font.weight: Font.Bold
                    horizontalAlignment: Text.AlignLeft
                    selectionColor: root.theme.colors.selection
                    selectedTextColor: root.theme.colors.textPrimary
                    clip: true
                    inputMethodHints: Qt.ImhFormattedNumbersOnly
                    maximumLength: 80
                    validator: RegularExpressionValidator {
                        regularExpression: /^[0-9]*\.?[0-9]*$/
                    }

                    Accessible.name: root.label

                    onTextEdited: {
                        if (!root.readOnly)
                            root.amountEdited(text)
                    }
                    onEditingFinished: {
                        if (!root.readOnly)
                            root.amountEditingFinished(text)
                    }
                }

                Text {
                    anchors.fill: parent
                    text: "0"
                    color: root.theme.colors.textPlaceholder
                    font: amountInput.font
                    visible: amountInput.text === "" && !amountInput.activeFocus
                    verticalAlignment: Text.AlignVCenter
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 6

                Loader {
                    active: root.adjustment !== null
                    visible: active
                    sourceComponent: root.adjustment
                    Layout.preferredWidth: active ? root.adjustmentWidth : 0
                    Layout.preferredHeight: active ? root.adjustmentHeight : 0
                }

                Button {
                    id: supportingAction

                    visible: root.supportingActionText.length > 0
                    enabled: visible && !root.readOnly
                    implicitWidth: 40
                    implicitHeight: 24
                    text: root.supportingActionText
                    hoverEnabled: true
                    Accessible.name: text
                    onClicked: root.supportingActionClicked()

                    contentItem: Text {
                        text: supportingAction.text
                        color: supportingAction.enabled
                               ? root.theme.colors.ctaBg
                               : root.theme.colors.textPlaceholder
                        font.pixelSize: 11
                        font.weight: Font.Bold
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }

                    background: Rectangle {
                        radius: 6
                        color: supportingAction.hovered || supportingAction.activeFocus
                               ? root.theme.colors.selection : "transparent"
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: root.supportingText
                    color: root.theme.colors.textSecondary
                    font.pixelSize: 11
                    horizontalAlignment: root.adjustment !== null
                                         || root.supportingActionText.length > 0
                                         ? Text.AlignRight : Text.AlignLeft
                    elide: Text.ElideRight
                    visible: text.length > 0
                }
            }
        }

        Loader {
            sourceComponent: root.accessory
            visible: status === Loader.Ready
            Layout.alignment: Qt.AlignTop
            Layout.topMargin: 17
            Layout.preferredWidth: root.accessoryWidth
            Layout.preferredHeight: root.accessoryHeight
        }
    }

    Loader {
        id: footerLoader

        active: root.footerActive
        visible: active
        sourceComponent: root.footer
        height: active ? root.footerHeight : 0
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        anchors.bottomMargin: active ? 12 : 0
    }

    Behavior on color {
        ColorAnimation { duration: 180 }
    }
}
