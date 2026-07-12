pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Controls
import Logos.Theme

FocusScope {
    id: root

    property var snapshot: ({})
    property bool open: false
    property bool busy: false

    signal canceled
    signal confirmed(var snapshot)

    visible: open
    focus: open
    z: 200

    Keys.onEscapePressed: function(event) {
        event.accepted = true
        if (!root.busy)
            root.cancel()
    }

    function openWithSnapshot(nextSnapshot) {
        root.snapshot = nextSnapshot || ({})
        root.open = true
        root.forceActiveFocus()
    }

    function cancel() {
        if (root.busy)
            return
        root.open = false
        root.canceled()
    }

    function confirm() {
        if (!root.busy)
            root.confirmed(root.snapshot)
    }

    function closeAfterSuccess() {
        root.open = false
        root.snapshot = ({})
    }

    function closeAfterFailure() {
        root.open = false
    }

    Rectangle {
        anchors.fill: parent
        color: "#B0000000"

        MouseArea {
            anchors.fill: parent
        }
    }

    Rectangle {
        anchors.centerIn: parent
        width: Math.min(520, parent.width - 32)
        implicitHeight: dialogContent.implicitHeight + 40
        radius: 8
        color: Theme.palette.backgroundElevated
        border.color: Theme.palette.borderSecondary
        border.width: 1

        ColumnLayout {
            id: dialogContent

            anchors.fill: parent
            anchors.margins: 20
            spacing: 14

            RowLayout {
                Layout.fillWidth: true

                Text {
                    text: qsTr("Confirm new position")
                    color: Theme.palette.text
                    font.pixelSize: 19
                    font.weight: Font.DemiBold
                    font.letterSpacing: 0
                    Layout.fillWidth: true
                }

                BusyIndicator {
                    running: root.busy
                    visible: running
                    implicitWidth: 24
                    implicitHeight: 24
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: summary.implicitHeight + 24
                radius: 6
                color: Theme.palette.backgroundTertiary

                ColumnLayout {
                    id: summary
                    anchors.fill: parent
                    anchors.margins: 12
                    spacing: 9

                    SummaryLine {
                        label: qsTr("Pair")
                        value: root.snapshot.pairText || "—"
                    }

                    SummaryLine {
                        label: qsTr("Action")
                        value: root.snapshot.instruction || "—"
                    }

                    SummaryLine {
                        label: qsTr("Fee")
                        value: root.snapshot.feeText || "—"
                    }

                    SummaryLine {
                        label: qsTr("Deposit")
                        value: qsTr("%1 + %2")
                               .arg(root.snapshot.depositAText || "—")
                               .arg(root.snapshot.depositBText || "—")
                    }

                    SummaryLine {
                        label: qsTr("Expected LP")
                        value: root.snapshot.expectedLpText || "—"
                    }
                }
            }

            Text {
                text: root.busy
                      ? qsTr("Waiting for wallet submission")
                      : qsTr("Your wallet will review and submit this transaction.")
                color: Theme.palette.textSecondary
                font.pixelSize: 12
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                LogosButton {
                    text: qsTr("Cancel")
                    enabled: !root.busy
                    Layout.fillWidth: true
                    Layout.minimumHeight: 44
                    radius: 6
                    onClicked: root.cancel()
                }

                LogosButton {
                    text: root.busy ? qsTr("Submitting…") : qsTr("Submit")
                    enabled: !root.busy
                    Layout.fillWidth: true
                    Layout.minimumHeight: 44
                    radius: 6
                    onClicked: root.confirm()
                }
            }
        }
    }

    component SummaryLine: RowLayout {
        required property string label
        required property string value
        Layout.fillWidth: true
        spacing: 12

        Text {
            text: parent.label
            color: Theme.palette.textSecondary
            font.pixelSize: 12
            Layout.fillWidth: true
        }

        Text {
            text: parent.value
            color: Theme.palette.text
            font.pixelSize: 12
            font.weight: Font.Medium
            horizontalAlignment: Text.AlignRight
            wrapMode: Text.WrapAnywhere
            Layout.maximumWidth: 320
        }
    }
}
