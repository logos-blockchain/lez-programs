/*
 * Historical-definition inspector. This is intentionally a read model: the
 * testnet snapshot and locally prepared drafts never trigger a chain query.
 */
pragma ComponentBehavior: Bound

import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: root

    property var store: null
    property string query: ""
    property string typeFilter: "all"
    property string selectedId: ""

    readonly property var definitions: store ? store.allDefinitions : []
    readonly property var filteredDefinitions: root.filterDefinitions()
    readonly property var selectedDefinition: store && selectedId.length > 0 ? store.findDefinition(selectedId) : null
    readonly property bool hasSelection: selectedDefinition !== null
    readonly property bool selectedIsFungible: hasSelection && selectedDefinition.type === "fungible"
    readonly property bool selectedIsNft: hasSelection && selectedDefinition.type === "nonFungible"

    function filterDefinitions() {
        var matches = store ? store.visibleDefinitions(query) : [];
        if (typeFilter === "all")
            return matches;

        var typed = [];
        for (var index = 0; index < matches.length; ++index) {
            if (matches[index].type === typeFilter)
                typed.push(matches[index]);
        }
        return typed;
    }

    function ensureSelection() {
        if (!store)
            return;

        var matches = filteredDefinitions;
        if (matches.length === 0) {
            selectedId = "";
            return;
        }

        for (var index = 0; index < matches.length; ++index) {
            if (matches[index].id === selectedId)
                return;
        }

        if (selectedId.length === 0 && typeFilter === "all" && query.length === 0) {
            var featured = store.findDefinition("3sG2zN3fAXBvs9mdgFHFiJFSFiwiYyaR5HUA3gZZTSXt");
            if (featured) {
                selectedId = featured.id;
                return;
            }
        }

        selectedId = matches[0].id;
    }

    function valueOrDash(value) {
        if (value === null || value === undefined || value === "")
            return "—";
        return String(value);
    }

    function sourceLabel(definition) {
        if (!definition)
            return "";
        return definition.source === "draft" ? qsTr("Local draft") : qsTr("Testnet snapshot · 12 Jul 2026");
    }

    function authorityTitle(definition) {
        if (!definition)
            return "—";
        if (definition.type === "nonFungible")
            return qsTr("Master holding");
        if (definition.authorityMode === "external")
            return qsTr("External authority");
        if (definition.authorityMode === "self")
            return qsTr("Self authority");
        if (definition.authorityMode === "renounced")
            return qsTr("Authority revoked");
        return qsTr("Fixed supply");
    }

    function authorityDescription(definition) {
        if (!definition)
            return "";
        if (definition.type === "nonFungible")
            return qsTr("Printing is controlled by the NFT master holding, not a definition authority.");
        if (definition.authorityMode === "external")
            return qsTr("A separate account can sign future fungible mints.");
        if (definition.authorityMode === "self")
            return qsTr("The definition account is the mint authority.");
        if (definition.authorityMode === "renounced")
            return qsTr("This historical definition exercised authority changes, then ended with no authority.");
        return qsTr("No mint authority is stored; the supply cannot be increased.");
    }

    Component.onCompleted: root.ensureSelection()
    onStoreChanged: root.ensureSelection()
    onFilteredDefinitionsChanged: root.ensureSelection()

    Rectangle {
        anchors.fill: parent
        color: "#151515"
    }

    Flickable {
        id: scroll

        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: content.implicitHeight + 48
        flickableDirection: Flickable.VerticalFlick

        ColumnLayout {
            id: content

            width: Math.max(280, Math.min(scroll.width - 40, 1440))
            x: Math.max(20, (scroll.width - width) / 2)
            y: 24
            spacing: 18

            RowLayout {
                Layout.fillWidth: true
                spacing: 18

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 5

                    Text {
                        color: "#E7E1D8"
                        font.pixelSize: 28
                        font.weight: Font.DemiBold
                        text: qsTr("Inspect token definitions")
                    }

                    Text {
                        Layout.fillWidth: true
                        color: "#A9A098"
                        font.pixelSize: 14
                        wrapMode: Text.Wrap
                        text: qsTr("Read the deployed definition state and locally prepared drafts. Testnet data is a historical reference, not a live chain response.")
                    }
                }

                Rectangle {
                    Layout.alignment: Qt.AlignTop
                    Layout.preferredHeight: 28
                    Layout.preferredWidth: snapshotLabel.implicitWidth + 18
                    radius: 14
                    color: "#182534"
                    border.color: "#40607A"
                    border.width: 1

                    Text {
                        id: snapshotLabel

                        anchors.centerIn: parent
                        color: "#BFD8F4"
                        font.pixelSize: 12
                        font.weight: Font.DemiBold
                        text: qsTr("Read-only snapshot")
                    }
                }
            }

            GridLayout {
                id: workbench

                readonly property int columnCount: content.width >= 1100 ? 2 : 1

                Layout.fillWidth: true
                columns: columnCount
                columnSpacing: 14
                rowSpacing: 14

                Rectangle {
                    id: indexPanel

                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    Layout.preferredWidth: workbench.columnCount === 2 ? 390 : 0
                    implicitHeight: indexContent.implicitHeight + 32
                    radius: 16
                    color: "#1B1B1B"
                    border.color: "#303030"
                    border.width: 1

                    ColumnLayout {
                        id: indexContent

                        anchors.fill: parent
                        anchors.margins: 16
                        spacing: 12

                        RowLayout {
                            Layout.fillWidth: true

                            Text {
                                color: "#E7E1D8"
                                font.pixelSize: 18
                                font.weight: Font.DemiBold
                                text: qsTr("Definition index")
                            }

                            Item {
                                Layout.fillWidth: true
                            }

                            Text {
                                color: "#8E8780"
                                font.pixelSize: 12
                                text: qsTr("%1 shown").arg(root.filteredDefinitions.length)
                            }
                        }

                        TextField {
                            id: searchField

                            Layout.fillWidth: true
                            Layout.preferredHeight: 42
                            Accessible.name: qsTr("Search token definitions")
                            color: "#E7E1D8"
                            font.pixelSize: 14
                            placeholderText: qsTr("Search name or definition account")
                            placeholderTextColor: "#8E8780"
                            selectByMouse: true
                            onTextChanged: root.query = text

                            background: Rectangle {
                                radius: 7
                                color: "#101010"
                                border.color: searchField.activeFocus ? "#F26A21" : "#343434"
                                border.width: 1
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 7

                            Button {
                                id: allFilterButton

                                Layout.preferredHeight: 30
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Show all token definitions")
                                text: qsTr("All")
                                onClicked: root.typeFilter = "all"

                                contentItem: Text {
                                    color: root.typeFilter === "all" ? "#F2D8C7" : "#A9A098"
                                    font.pixelSize: 12
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: allFilterButton.text
                                }

                                background: Rectangle {
                                    radius: 6
                                    color: root.typeFilter === "all" ? "#211914" : "#101010"
                                    border.color: allFilterButton.activeFocus ? "#FFB26B" : root.typeFilter === "all" ? "#F26A21" : "#343434"
                                    border.width: allFilterButton.activeFocus ? 2 : 1
                                }
                            }

                            Button {
                                id: fungibleFilterButton

                                Layout.preferredHeight: 30
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Show fungible definitions")
                                text: qsTr("Fungible")
                                onClicked: root.typeFilter = "fungible"

                                contentItem: Text {
                                    color: root.typeFilter === "fungible" ? "#F2D8C7" : "#A9A098"
                                    font.pixelSize: 12
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: fungibleFilterButton.text
                                }

                                background: Rectangle {
                                    radius: 6
                                    color: root.typeFilter === "fungible" ? "#211914" : "#101010"
                                    border.color: fungibleFilterButton.activeFocus ? "#FFB26B" : root.typeFilter === "fungible" ? "#F26A21" : "#343434"
                                    border.width: fungibleFilterButton.activeFocus ? 2 : 1
                                }
                            }

                            Button {
                                id: nftFilterButton

                                Layout.preferredHeight: 30
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Show non-fungible definitions")
                                text: qsTr("NFT")
                                onClicked: root.typeFilter = "nonFungible"

                                contentItem: Text {
                                    color: root.typeFilter === "nonFungible" ? "#BFD8F4" : "#A9A098"
                                    font.pixelSize: 12
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: nftFilterButton.text
                                }

                                background: Rectangle {
                                    radius: 6
                                    color: root.typeFilter === "nonFungible" ? "#182534" : "#101010"
                                    border.color: nftFilterButton.activeFocus ? "#FFB26B" : root.typeFilter === "nonFungible" ? "#40607A" : "#343434"
                                    border.width: nftFilterButton.activeFocus ? 2 : 1
                                }
                            }

                            Item {
                                Layout.fillWidth: true
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: "#303030"
                        }

                        Column {
                            id: definitionList

                            Layout.fillWidth: true
                            spacing: 7

                            Repeater {
                                model: root.filteredDefinitions

                                delegate: Rectangle {
                                    id: definitionRow

                                    required property var modelData

                                    width: definitionList.width
                                    height: 70
                                    radius: 9
                                    color: root.selectedId === definitionRow.modelData.id ? (definitionRow.modelData.type === "fungible" ? "#211914" : "#182534") : rowPointer.containsMouse ? "#222222" : "#101010"
                                    border.color: root.selectedId === definitionRow.modelData.id || definitionRow.activeFocus ? (definitionRow.modelData.type === "fungible" ? "#F26A21" : "#40607A") : "#343434"
                                    border.width: 1
                                    activeFocusOnTab: true
                                    Accessible.name: qsTr("Inspect %1 definition").arg(definitionRow.modelData.name)
                                    Accessible.role: Accessible.Button
                                    Accessible.onPressAction: definitionRow.inspectDefinition()

                                    function inspectDefinition() {
                                        root.selectedId = definitionRow.modelData.id;
                                    }

                                    Keys.onReturnPressed: definitionRow.inspectDefinition()
                                    Keys.onSpacePressed: definitionRow.inspectDefinition()

                                    MouseArea {
                                        id: rowPointer

                                        anchors.fill: parent
                                        hoverEnabled: true
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: definitionRow.inspectDefinition()
                                    }

                                    RowLayout {
                                        anchors.fill: parent
                                        anchors.margins: 11
                                        spacing: 10

                                        Rectangle {
                                            Layout.preferredHeight: 30
                                            Layout.preferredWidth: rowKind.implicitWidth + 14
                                            radius: 15
                                            color: definitionRow.modelData.type === "fungible" ? "#2D211A" : "#182534"
                                            border.color: definitionRow.modelData.type === "fungible" ? "#6A4329" : "#40607A"
                                            border.width: 1

                                            Text {
                                                id: rowKind

                                                anchors.centerIn: parent
                                                color: definitionRow.modelData.type === "fungible" ? "#F2D8C7" : "#BFD8F4"
                                                font.pixelSize: 11
                                                font.weight: Font.DemiBold
                                                text: definitionRow.modelData.type === "fungible" ? qsTr("FT") : qsTr("NFT")
                                            }
                                        }

                                        ColumnLayout {
                                            Layout.fillWidth: true
                                            spacing: 2

                                            Text {
                                                Layout.fillWidth: true
                                                color: "#E7E1D8"
                                                elide: Text.ElideRight
                                                font.pixelSize: 14
                                                font.weight: Font.DemiBold
                                                text: definitionRow.modelData.name
                                            }

                                            Text {
                                                Layout.fillWidth: true
                                                color: "#8E8780"
                                                elide: Text.ElideMiddle
                                                font.family: "monospace"
                                                font.pixelSize: 11
                                                text: root.store ? root.store.shortAddress(definitionRow.modelData.definitionId) : definitionRow.modelData.definitionId
                                            }
                                        }

                                        Text {
                                            color: definitionRow.modelData.source === "draft" ? "#78C88D" : "#A9A098"
                                            font.pixelSize: 11
                                            text: definitionRow.modelData.source === "draft" ? qsTr("Draft") : qsTr("Testnet")
                                        }
                                    }
                                }
                            }

                            Text {
                                width: parent.width
                                visible: root.filteredDefinitions.length === 0
                                color: "#A9A098"
                                font.pixelSize: 13
                                horizontalAlignment: Text.AlignHCenter
                                text: qsTr("No definition matches this search.")
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: indexFootnote.implicitHeight + 20
                            radius: 8
                            color: "#181818"
                            border.color: "#303030"
                            border.width: 1

                            Text {
                                id: indexFootnote

                                anchors.fill: parent
                                anchors.margins: 10
                                color: "#8E8780"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: qsTr("A prepared draft joins this index locally; it is not an account, does not reserve an address, and is never sent to testnet.")
                            }
                        }
                    }
                }

                Rectangle {
                    id: detailPanel

                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    implicitHeight: detailContent.implicitHeight + 32
                    radius: 16
                    color: "#1B1B1B"
                    border.color: "#303030"
                    border.width: 1

                    ColumnLayout {
                        id: detailContent

                        anchors.fill: parent
                        anchors.margins: 16
                        spacing: 14

                        Item {
                            Layout.fillWidth: true
                            Layout.preferredHeight: root.hasSelection ? headline.implicitHeight : emptyState.implicitHeight

                            RowLayout {
                                id: headline

                                anchors.fill: parent
                                visible: root.hasSelection
                                spacing: 10

                                Rectangle {
                                    Layout.alignment: Qt.AlignTop
                                    Layout.preferredHeight: 30
                                    Layout.preferredWidth: selectedKind.implicitWidth + 16
                                    radius: 15
                                    color: root.selectedIsFungible ? "#2D211A" : "#182534"
                                    border.color: root.selectedIsFungible ? "#6A4329" : "#40607A"
                                    border.width: 1

                                    Text {
                                        id: selectedKind

                                        anchors.centerIn: parent
                                        color: root.selectedIsFungible ? "#F2D8C7" : "#BFD8F4"
                                        font.pixelSize: 12
                                        font.weight: Font.DemiBold
                                        text: root.selectedIsFungible ? qsTr("Fungible") : qsTr("NFT collection")
                                    }
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 3

                                    Text {
                                        Layout.fillWidth: true
                                        color: "#E7E1D8"
                                        elide: Text.ElideRight
                                        font.pixelSize: 22
                                        font.weight: Font.DemiBold
                                        text: root.hasSelection ? root.selectedDefinition.name : ""
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        color: root.hasSelection && root.selectedDefinition.source === "draft" ? "#78C88D" : "#A9A098"
                                        font.pixelSize: 12
                                        text: root.hasSelection ? root.sourceLabel(root.selectedDefinition) : ""
                                    }
                                }
                            }

                            Text {
                                id: emptyState

                                anchors.fill: parent
                                visible: !root.hasSelection
                                color: "#A9A098"
                                font.pixelSize: 15
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                                text: qsTr("Choose a definition from the index to inspect it.")
                            }
                        }

                        GridLayout {
                            id: summaryGrid

                            Layout.fillWidth: true
                            visible: root.hasSelection
                            Layout.preferredHeight: visible ? implicitHeight : 0
                            columns: detailContent.width >= 800 ? 3 : detailContent.width >= 500 ? 2 : 1
                            columnSpacing: 9
                            rowSpacing: 9

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 80
                                radius: 10
                                color: "#101010"
                                border.color: "#343434"
                                border.width: 1

                                ColumnLayout {
                                    anchors.fill: parent
                                    anchors.margins: 11
                                    spacing: 3

                                    Text {
                                        color: "#8E8780"
                                        font.pixelSize: 11
                                        text: qsTr("Stored definition")
                                    }

                                    Text {
                                        color: "#E7E1D8"
                                        font.pixelSize: 15
                                        font.weight: Font.DemiBold
                                        text: root.selectedIsFungible ? qsTr("Fungible token") : qsTr("Non-fungible token")
                                    }

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 11
                                        text: root.selectedIsFungible ? qsTr("total_supply") : qsTr("printable_supply")
                                    }
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 80
                                radius: 10
                                color: "#101010"
                                border.color: "#343434"
                                border.width: 1

                                ColumnLayout {
                                    anchors.fill: parent
                                    anchors.margins: 11
                                    spacing: 3

                                    Text {
                                        color: "#8E8780"
                                        font.pixelSize: 11
                                        text: root.selectedIsFungible ? qsTr("Current raw supply") : qsTr("Printable supply")
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        color: "#E7E1D8"
                                        elide: Text.ElideMiddle
                                        font.family: "monospace"
                                        font.pixelSize: 14
                                        font.weight: Font.DemiBold
                                        text: !root.hasSelection ? "" : root.selectedIsFungible ? root.valueOrDash(root.selectedDefinition.rawSupply) : root.valueOrDash(root.selectedDefinition.printableCopies)
                                    }

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 11
                                        text: root.selectedIsFungible ? qsTr("raw u128") : qsTr("master starts at this balance")
                                    }
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 80
                                radius: 10
                                color: "#101010"
                                border.color: "#343434"
                                border.width: 1

                                ColumnLayout {
                                    anchors.fill: parent
                                    anchors.margins: 11
                                    spacing: 3

                                    Text {
                                        color: "#8E8780"
                                        font.pixelSize: 11
                                        text: root.selectedIsNft ? qsTr("Control") : qsTr("Mint authority")
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        color: root.hasSelection && root.selectedDefinition.authorityMode === "renounced" ? "#78C88D" : "#E7E1D8"
                                        elide: Text.ElideRight
                                        font.pixelSize: 15
                                        font.weight: Font.DemiBold
                                        text: root.authorityTitle(root.selectedDefinition)
                                    }

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 11
                                        text: root.hasSelection && root.selectedDefinition.metadataStandard ? qsTr("%1 metadata").arg(root.selectedDefinition.metadataStandard) : qsTr("No metadata linked")
                                    }
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            visible: root.hasSelection
                            Layout.preferredHeight: visible ? definitionRecord.implicitHeight + 26 : 0
                            radius: 10
                            color: "#101010"
                            border.color: "#343434"
                            border.width: 1

                            ColumnLayout {
                                id: definitionRecord

                                anchors.fill: parent
                                anchors.margins: 13
                                spacing: 8

                                Text {
                                    color: "#E7E1D8"
                                    font.pixelSize: 16
                                    font.weight: Font.DemiBold
                                    text: qsTr("Definition record")
                                }

                                Text {
                                    color: "#8E8780"
                                    font.pixelSize: 11
                                    text: qsTr("DEFINITION ACCOUNT")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    elide: Text.ElideMiddle
                                    font.family: "monospace"
                                    font.pixelSize: 13
                                    text: root.hasSelection ? root.selectedDefinition.definitionId : ""
                                }

                                Text {
                                    Layout.fillWidth: true
                                    visible: root.hasSelection && root.selectedDefinition.definitionHex
                                    Layout.preferredHeight: visible ? implicitHeight : 0
                                    color: "#8E8780"
                                    elide: Text.ElideMiddle
                                    font.family: "monospace"
                                    font.pixelSize: 11
                                    text: root.hasSelection ? qsTr("hex %1").arg(root.selectedDefinition.definitionHex) : ""
                                }

                                Rectangle {
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 1
                                    color: "#303030"
                                }

                                GridLayout {
                                    Layout.fillWidth: true
                                    columns: detailContent.width >= 720 ? 2 : 1
                                    columnSpacing: 16
                                    rowSpacing: 7

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        visible: root.selectedIsFungible
                                        Layout.preferredHeight: visible ? implicitHeight : 0
                                        spacing: 3

                                        Text {
                                            color: "#8E8780"
                                            font.pixelSize: 11
                                            text: qsTr("DISPLAYED SUPPLY")
                                        }

                                        Text {
                                            color: "#E7E1D8"
                                            font.pixelSize: 15
                                            font.weight: Font.Medium
                                            text: root.hasSelection ? root.valueOrDash(root.selectedDefinition.displaySupply) : ""
                                        }

                                        Text {
                                            color: "#A9A098"
                                            font.pixelSize: 12
                                            text: root.hasSelection ? qsTr("UI inference: %1 decimal places; not stored in this definition.").arg(root.valueOrDash(root.selectedDefinition.inferredDecimals)) : ""
                                        }
                                    }

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        visible: root.selectedIsNft
                                        Layout.preferredHeight: visible ? implicitHeight : 0
                                        spacing: 3

                                        Text {
                                            color: "#8E8780"
                                            font.pixelSize: 11
                                            text: qsTr("NFT CREATION PATH")
                                        }

                                        Text {
                                            color: "#E7E1D8"
                                            font.family: "monospace"
                                            font.pixelSize: 14
                                            font.weight: Font.Medium
                                            text: root.hasSelection ? root.selectedDefinition.instruction : ""
                                        }

                                        Text {
                                            color: "#A9A098"
                                            font.pixelSize: 12
                                            text: qsTr("NFT creation always links metadata and creates one master holding.")
                                        }
                                    }

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 3

                                        Text {
                                            color: "#8E8780"
                                            font.pixelSize: 11
                                            text: qsTr("CREATION INSTRUCTION")
                                        }

                                        Text {
                                            color: "#F2D8C7"
                                            font.family: "monospace"
                                            font.pixelSize: 14
                                            font.weight: Font.Medium
                                            text: root.hasSelection ? root.selectedDefinition.instruction : ""
                                        }

                                        Text {
                                            color: "#A9A098"
                                            font.pixelSize: 12
                                            text: root.selectedIsFungible && root.selectedDefinition.metadataId ? qsTr("Metadata-backed fungible creation.") : root.selectedIsFungible ? qsTr("Plain fungible creation.") : qsTr("Typed metadata creation is required.")
                                        }
                                    }
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            visible: root.hasSelection
                            Layout.preferredHeight: visible ? authorityRecord.implicitHeight + 26 : 0
                            radius: 10
                            color: root.selectedIsNft ? "#181D25" : "#101010"
                            border.color: root.selectedIsNft ? "#31435D" : "#343434"
                            border.width: 1

                            ColumnLayout {
                                id: authorityRecord

                                anchors.fill: parent
                                anchors.margins: 13
                                spacing: 8

                                Text {
                                    color: "#E7E1D8"
                                    font.pixelSize: 16
                                    font.weight: Font.DemiBold
                                    text: root.selectedIsNft ? qsTr("Master holding and print control") : qsTr("Mint authority")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#A9A098"
                                    font.pixelSize: 13
                                    wrapMode: Text.Wrap
                                    text: root.hasSelection ? root.authorityDescription(root.selectedDefinition) : ""
                                }

                                Text {
                                    Layout.fillWidth: true
                                    visible: root.hasSelection && root.selectedIsFungible && !!root.selectedDefinition.authority
                                    Layout.preferredHeight: visible ? implicitHeight : 0
                                    color: "#E7E1D8"
                                    elide: Text.ElideMiddle
                                    font.family: "monospace"
                                    font.pixelSize: 13
                                    text: root.hasSelection ? root.selectedDefinition.authority : ""
                                }

                                Text {
                                    Layout.fillWidth: true
                                    visible: root.hasSelection && root.selectedIsFungible && !!root.selectedDefinition.authorityLabel
                                    Layout.preferredHeight: visible ? implicitHeight : 0
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: root.hasSelection ? root.selectedDefinition.authorityLabel : ""
                                }

                                Text {
                                    Layout.fillWidth: true
                                    visible: root.hasSelection && root.selectedIsFungible && root.selectedDefinition.authorityMode === "renounced"
                                    Layout.preferredHeight: visible ? implicitHeight : 0
                                    color: "#78C88D"
                                    font.pixelSize: 12
                                    wrapMode: Text.Wrap
                                    text: root.hasSelection ? qsTr("Initial self authority: %1").arg(root.selectedDefinition.initialAuthority) : ""
                                }

                                GridLayout {
                                    Layout.fillWidth: true
                                    visible: root.selectedIsNft
                                    Layout.preferredHeight: visible ? implicitHeight : 0
                                    columns: detailContent.width >= 720 ? 2 : 1
                                    columnSpacing: 16
                                    rowSpacing: 9

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 3

                                        Text {
                                            color: "#8E8780"
                                            font.pixelSize: 11
                                            text: qsTr("MASTER HOLDING")
                                        }

                                        Text {
                                            Layout.fillWidth: true
                                            color: "#E7E1D8"
                                            elide: Text.ElideMiddle
                                            font.family: "monospace"
                                            font.pixelSize: 13
                                            text: root.hasSelection ? root.valueOrDash(root.selectedDefinition.masterHolding) : ""
                                        }
                                    }

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 3

                                        Text {
                                            color: "#8E8780"
                                            font.pixelSize: 11
                                            text: qsTr("CURRENT MASTER PRINT BALANCE")
                                        }

                                        Text {
                                            color: "#BFD8F4"
                                            font.pixelSize: 15
                                            font.weight: Font.Medium
                                            text: root.hasSelection && root.selectedDefinition.holding ? root.valueOrDash(root.selectedDefinition.holding.printBalance) : "—"
                                        }
                                    }
                                }

                                Text {
                                    Layout.fillWidth: true
                                    visible: root.selectedIsNft
                                    Layout.preferredHeight: visible ? implicitHeight : 0
                                    color: "#BFD8F4"
                                    font.pixelSize: 12
                                    wrapMode: Text.Wrap
                                    text: qsTr("A print requires master balance greater than 1 and leaves one master unit reserved. Printed copies do not restore print capacity when burned.")
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            visible: root.hasSelection
                            Layout.preferredHeight: visible ? holdingsRecord.implicitHeight + 26 : 0
                            radius: 10
                            color: "#101010"
                            border.color: "#343434"
                            border.width: 1

                            ColumnLayout {
                                id: holdingsRecord

                                anchors.fill: parent
                                anchors.margins: 13
                                spacing: 9

                                Text {
                                    color: "#E7E1D8"
                                    font.pixelSize: 16
                                    font.weight: Font.DemiBold
                                    text: root.selectedIsNft ? qsTr("Observed NFT holdings") : qsTr("Observed token holdings")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    wrapMode: Text.Wrap
                                    text: root.hasSelection && root.selectedDefinition.source === "draft" ? qsTr("A prepared draft has an intended initial holding target but no observed account state.") : qsTr("Balances and ownership shown here belong to the supplied historical snapshot.")
                                }

                                Column {
                                    id: holdingList

                                    Layout.fillWidth: true
                                    spacing: 7
                                    visible: root.hasSelection && root.selectedDefinition.holdings && root.selectedDefinition.holdings.length > 0

                                    Repeater {
                                        model: root.hasSelection && root.selectedDefinition.holdings ? root.selectedDefinition.holdings : []

                                        delegate: Rectangle {
                                            id: holdingRow

                                            required property var modelData

                                            width: holdingList.width
                                            height: 62
                                            radius: 8
                                            color: holdingRow.modelData.role === "nftMaster" ? "#181D25" : "#181818"
                                            border.color: holdingRow.modelData.role === "nftMaster" ? "#31435D" : "#303030"
                                            border.width: 1

                                            RowLayout {
                                                anchors.fill: parent
                                                anchors.margins: 10
                                                spacing: 10

                                                ColumnLayout {
                                                    Layout.fillWidth: true
                                                    spacing: 2

                                                    Text {
                                                        Layout.fillWidth: true
                                                        color: "#E7E1D8"
                                                        elide: Text.ElideMiddle
                                                        font.family: "monospace"
                                                        font.pixelSize: 12
                                                        text: holdingRow.modelData.id
                                                    }

                                                    Text {
                                                        color: "#A9A098"
                                                        font.pixelSize: 11
                                                        text: holdingRow.modelData.role === "nftMaster" ? qsTr("NFT master holding") : holdingRow.modelData.role === "nftPrintedCopy" ? (holdingRow.modelData.owned ? qsTr("Printed copy · owned") : qsTr("Printed copy · unowned")) : qsTr("%1 · %2").arg(holdingRow.modelData.wallet).arg(holdingRow.modelData.role)
                                                    }
                                                }

                                                ColumnLayout {
                                                    Layout.alignment: Qt.AlignRight
                                                    spacing: 2

                                                    Text {
                                                        color: holdingRow.modelData.printBalance !== undefined ? "#BFD8F4" : "#E7E1D8"
                                                        font.family: "monospace"
                                                        font.pixelSize: 12
                                                        font.weight: Font.Medium
                                                        text: holdingRow.modelData.printBalance !== undefined ? root.valueOrDash(holdingRow.modelData.printBalance) : holdingRow.modelData.rawBalance !== undefined ? root.valueOrDash(holdingRow.modelData.rawBalance) : ""
                                                    }

                                                    Text {
                                                        visible: holdingRow.modelData.displayBalance !== undefined || holdingRow.modelData.printBalance !== undefined
                                                        color: "#8E8780"
                                                        font.pixelSize: 11
                                                        text: holdingRow.modelData.printBalance !== undefined ? qsTr("print balance") : holdingRow.modelData.displayBalance
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                Text {
                                    Layout.fillWidth: true
                                    visible: root.hasSelection && (!root.selectedDefinition.holdings || root.selectedDefinition.holdings.length === 0)
                                    Layout.preferredHeight: visible ? implicitHeight : 0
                                    color: "#E7E1D8"
                                    elide: Text.ElideMiddle
                                    font.family: "monospace"
                                    font.pixelSize: 13
                                    text: root.hasSelection ? root.valueOrDash(root.selectedDefinition.holdingId) : ""
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            visible: root.hasSelection && !!root.selectedDefinition.metadataId
                            Layout.preferredHeight: visible ? metadataRecord.implicitHeight + 26 : 0
                            radius: 10
                            color: "#101010"
                            border.color: "#343434"
                            border.width: 1

                            ColumnLayout {
                                id: metadataRecord

                                anchors.fill: parent
                                anchors.margins: 13
                                spacing: 8

                                RowLayout {
                                    Layout.fillWidth: true

                                    Text {
                                        color: "#E7E1D8"
                                        font.pixelSize: 16
                                        font.weight: Font.DemiBold
                                        text: qsTr("Linked metadata")
                                    }

                                    Item {
                                        Layout.fillWidth: true
                                    }

                                    Rectangle {
                                        Layout.preferredHeight: 23
                                        Layout.preferredWidth: metadataStandard.implicitWidth + 14
                                        radius: 12
                                        color: "#211914"
                                        border.color: "#49301F"
                                        border.width: 1

                                        Text {
                                            id: metadataStandard

                                            anchors.centerIn: parent
                                            color: "#F2D8C7"
                                            font.pixelSize: 11
                                            font.weight: Font.DemiBold
                                            text: root.hasSelection ? root.valueOrDash(root.selectedDefinition.metadataStandard) : ""
                                        }
                                    }
                                }

                                Text {
                                    color: "#8E8780"
                                    font.pixelSize: 11
                                    text: qsTr("METADATA ACCOUNT")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    elide: Text.ElideMiddle
                                    font.family: "monospace"
                                    font.pixelSize: 13
                                    text: root.hasSelection ? root.valueOrDash(root.selectedDefinition.metadataId) : ""
                                }

                                Text {
                                    Layout.fillWidth: true
                                    visible: root.hasSelection && !!root.selectedDefinition.description
                                    Layout.preferredHeight: visible ? implicitHeight : 0
                                    color: "#A9A098"
                                    font.pixelSize: 13
                                    wrapMode: Text.Wrap
                                    text: root.hasSelection ? root.selectedDefinition.description : ""
                                }

                                Rectangle {
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 1
                                    color: "#303030"
                                }

                                Text {
                                    color: "#8E8780"
                                    font.pixelSize: 11
                                    text: qsTr("URI")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    font.family: "monospace"
                                    font.pixelSize: 11
                                    wrapMode: Text.WrapAnywhere
                                    text: root.hasSelection ? root.valueOrDash(root.selectedDefinition.metadataUri) : ""
                                }

                                Text {
                                    color: "#8E8780"
                                    font.pixelSize: 11
                                    text: qsTr("CREATORS")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    font.pixelSize: 13
                                    wrapMode: Text.WrapAnywhere
                                    text: root.hasSelection ? root.valueOrDash(root.selectedDefinition.creators) : ""
                                }

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Stored primary_sale_date starts at 0. The token program does not validate URI content or metadata schema.")
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            visible: root.hasSelection && !root.selectedDefinition.metadataId
                            Layout.preferredHeight: visible ? metadataAbsent.implicitHeight + 22 : 0
                            radius: 9
                            color: "#181818"
                            border.color: "#303030"
                            border.width: 1

                            Text {
                                id: metadataAbsent

                                anchors.fill: parent
                                anchors.margins: 11
                                color: "#A9A098"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: qsTr("No metadata account is linked to this definition. Metadata is optional for fungibles and required for non-fungible definitions.")
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            visible: root.hasSelection
                            Layout.preferredHeight: visible ? contractNotice.implicitHeight + 22 : 0
                            radius: 9
                            color: "#211914"
                            border.color: "#49301F"
                            border.width: 1

                            Text {
                                id: contractNotice

                                anchors.fill: parent
                                anchors.margins: 11
                                color: "#F2D8C7"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: root.selectedIsFungible ? qsTr("The Token Program stores raw supply and optional authority/metadata links. Decimals, symbol, image, royalties, collection, and mutable metadata are not definition settings.") : qsTr("The Token Program stores the NFT name, printable supply, and required metadata link. It has no NFT mint authority or configurable royalty, collection, image, or mutable-metadata setting.")
                            }
                        }
                    }
                }
            }
        }
    }
}
