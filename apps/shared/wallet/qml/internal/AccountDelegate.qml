import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

ItemDelegate {
    id: root

    required property int index
    required property string name
    required property string alias
    required property string address
    required property string displayAddress
    required property string balance
    required property bool isPublic
    required property string kind
    required property string section
    required property string programName
    required property string accountType
    required property string decodedData
    required property string visibility
    required property bool canBePrimary
    required property bool isPrimary

    signal copyRequested(string text)
    signal makePrimaryRequested(string address)
    signal renameRequested(string address, string alias)

    leftPadding: 12
    rightPadding: 8
    topPadding: 10
    bottomPadding: 10
    enabled: root.section !== "hidden"
    Accessible.name: root.isPrimary
        ? qsTr("%1, primary account").arg(root.name)
        : root.name

    function kindLabel() {
        if (root.kind === "user")
            return qsTr("User")
        if (root.kind === "private")
            return qsTr("Account")
        if (root.accountType.length > 0)
            return root.accountType
        return root.kind === "unknown" ? qsTr("Unknown") : qsTr("Program")
    }

    background: Rectangle {
        color: root.isPrimary || root.hovered ? "#27272a" : "#18181b"
        radius: 8
        border.width: root.activeFocus || root.isPrimary ? 1 : 0
        border.color: root.isPrimary ? "#f59e0b" : "#52525b"
    }

    contentItem: ColumnLayout {
        spacing: 7

        RowLayout {
            Layout.fillWidth: true
            spacing: 7

            Label {
                Layout.fillWidth: true
                text: root.name
                color: "#fafafa"
                font.bold: true
                elide: Text.ElideRight
            }

            Label {
                visible: root.isPrimary
                text: qsTr("Primary")
                color: "#fbbf24"
                font.pixelSize: 11
                font.bold: true
            }

            Label {
                text: root.kindLabel()
                color: "#a1a1aa"
                font.pixelSize: 11
            }

            Label {
                text: root.visibility === "private" ? qsTr("Private") : qsTr("Public")
                color: root.visibility === "private" ? "#c4b5fd" : "#93c5fd"
                font.pixelSize: 11
            }
        }

        Label {
            objectName: "walletProgramName"
            visible: root.section === "advanced" && root.programName.length > 0
            Layout.fillWidth: true
            text: qsTr("Program: %1").arg(root.programName)
            color: "#a1a1aa"
            font.pixelSize: 11
            elide: Text.ElideRight
        }

        ColumnLayout {
            visible: root.section === "advanced" && root.decodedData.length > 0
            Layout.fillWidth: true
            spacing: 4

            Label {
                objectName: "walletDecodedDataLabel"
                text: qsTr("Decoded data")
                color: "#a1a1aa"
                font.pixelSize: 11
            }

            Rectangle {
                objectName: "walletDecodedDataBox"
                Layout.fillWidth: true
                implicitHeight: decodedDataText.implicitHeight + 16
                color: "#18181b"
                radius: 6
                border.width: 1
                border.color: "#3f3f46"

                Text {
                    id: decodedDataText
                    objectName: "walletDecodedData"
                    anchors.fill: parent
                    anchors.margins: 8
                    text: root.decodedData
                    color: "#d4d4d8"
                    font.family: "monospace"
                    font.pixelSize: 10
                    textFormat: Text.PlainText
                    wrapMode: Text.WrapAnywhere
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 4

            Label {
                Layout.fillWidth: true
                text: root.displayAddress
                color: "#71717a"
                font.family: "monospace"
                font.pixelSize: 11
                elide: Text.ElideMiddle
            }

            CopyButton {
                visible: root.displayAddress.length > 0
                onCopyRequested: root.copyRequested(root.displayAddress)
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            Button {
                objectName: "walletRenameButton"
                text: qsTr("Rename")
                flat: true
                onClicked: root.renameRequested(root.address, root.alias)
            }

            Item { Layout.fillWidth: true }

            Button {
                objectName: "walletMakePrimaryButton"
                visible: root.canBePrimary && !root.isPrimary
                text: qsTr("Make primary")
                flat: true
                onClicked: root.makePrimaryRequested(root.address)
            }
        }
    }

    onClicked: {
        if (root.canBePrimary && !root.isPrimary)
            root.makePrimaryRequested(root.address)
    }
}
