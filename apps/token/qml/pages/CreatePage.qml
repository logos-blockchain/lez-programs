/*
 * THESIS: A definition workbench makes the token program's few irreversible
 * choices legible before a user ever signs. It refuses a generic dashboard in
 * favor of one live creation sheet, one outcome preview, and one readiness rail.
 * OWN-WORLD: Existing LEZ charcoal surfaces, warm text, thin borders, and amber
 * only for selection and the single primary action.
 * STORY: An operator chooses fungible or NFT, exposes every protocol setting,
 * sees exact resulting state, then prepares—not submits—a definition draft.
 * FIRST VIEWPORT: Input sheet left, state preview center, signer/readiness rail
 * right; narrow windows stack those regions in reading order.
 * FORM: Grounded surface candidate 3, seed eda5e259. FINISH: unreviewed and
 * undocumented is unfinished; this build ends with the finish review, the
 * verdict, and DESIGN.md.
 */
import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: root

    property var store: null
    property int tokenKind: 0
    property int authorityMode: 0
    property bool metadataEnabled: false
    property int metadataStandard: 0
    property bool prepared: false
    property string preparedMessage: ""

    readonly property bool isFungible: tokenKind === 0
    readonly property bool requiresMetadata: !isFungible
    readonly property bool hasMetadata: requiresMetadata || metadataEnabled
    readonly property string maximumU128: "340282366920938463463374607431768211455"
    readonly property string instructionName: isFungible && !hasMetadata ? "new_fungible_definition" : "new_definition_with_metadata"
    readonly property string supplyLabel: isFungible ? qsTr("Initial raw supply") : qsTr("Printable supply")
    readonly property string supplyValue: supplyField.text.length > 0 ? supplyField.text : qsTr("Not set")
    readonly property string authorityValue: !isFungible ? qsTr("Not applicable to NFT definitions") : authorityMode === 0 ? qsTr("None — fixed supply") : authorityMode === 1 ? qsTr("Definition account — self authority") : externalAuthorityField.text.length > 0 ? externalAuthorityField.text : qsTr("External authority required")
    readonly property bool validSupply: isUnsignedU128(supplyField.text)
    readonly property bool validDefinitionTarget: isAccountId(definitionTargetField.text)
    readonly property bool validHoldingTarget: isAccountId(holdingTargetField.text)
    readonly property bool validMetadataTarget: !hasMetadata || isAccountId(metadataTargetField.text)
    readonly property bool validExternalAuthority: !isFungible || authorityMode !== 2 || (isAccountId(externalAuthorityField.text) && externalAuthorityField.text !== "11111111111111111111111111111111")
    readonly property bool canPrepare: validSupply && validDefinitionTarget && validHoldingTarget && validMetadataTarget && validExternalAuthority

    function isUnsignedU128(value) {
        if (!/^[0-9]+$/.test(value))
            return false;

        var normalized = value.replace(/^0+(?=\d)/, "");
        if (normalized.length < maximumU128.length)
            return true;
        if (normalized.length > maximumU128.length)
            return false;
        return normalized <= maximumU128;
    }

    function isAccountId(value) {
        return /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(value);
    }

    function setPattern(pattern) {
        prepared = false;
        preparedMessage = "";

        if (pattern === "fixed") {
            tokenKind = 0;
            authorityMode = 0;
            metadataEnabled = false;
            nameField.text = qsTr("Example fixed supply");
            supplyField.text = "7654321";
            return;
        }

        if (pattern === "metadata") {
            tokenKind = 0;
            authorityMode = 2;
            metadataEnabled = true;
            metadataStandard = 1;
            nameField.text = qsTr("Example metadata token");
            supplyField.text = "10000000000000000000000000";
            metadataUriField.text = "data:application/json;base64,...";
            creatorsField.text = qsTr("Creator or authority label");
            return;
        }

        tokenKind = 1;
        metadataEnabled = true;
        metadataStandard = 1;
        nameField.text = qsTr("Example collection");
        supplyField.text = "64";
        metadataUriField.text = "data:application/json;base64,...";
        creatorsField.text = qsTr("Master-holding creator");
    }

    function prepareDefinition() {
        if (!canPrepare)
            return;

        var draft = {
            id: "draft-" + Date.now(),
            name: nameField.text.length > 0 ? nameField.text : qsTr("Untitled definition"),
            type: isFungible ? "fungible" : "nonFungible",
            definitionId: definitionTargetField.text,
            holdingId: holdingTargetField.text,
            metadataId: hasMetadata ? metadataTargetField.text : "",
            rawSupply: supplyField.text,
            displaySupply: supplyField.text,
            inferredDecimals: "",
            authorityMode: isFungible ? (authorityMode === 0 ? "fixed" : authorityMode === 1 ? "self" : "external") : "masterHolding",
            authority: isFungible && authorityMode === 2 ? externalAuthorityField.text : authorityMode === 1 ? definitionTargetField.text : "",
            metadataStandard: hasMetadata ? (metadataStandard === 0 ? "Simple" : "Expanded") : "",
            metadataUri: hasMetadata ? metadataUriField.text : "",
            creators: hasMetadata ? creatorsField.text : "",
            description: qsTr("Prepared locally. No token account or transaction was created."),
            source: "draft",
            instruction: instructionName,
            printableCopies: !isFungible ? supplyField.text : "",
            masterHolding: !isFungible ? holdingTargetField.text : ""
        };

        if (store && store.addDraft)
            store.addDraft(draft);

        prepared = true;
        preparedMessage = qsTr("Draft prepared locally. Switch to Inspect to review it alongside the testnet fixtures.");
    }

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
                        text: qsTr("Create token definition")
                    }

                    Text {
                        Layout.fillWidth: true
                        color: "#A9A098"
                        font.pixelSize: 14
                        wrapMode: Text.Wrap
                        text: qsTr("Prepare the exact definition shape first. This prototype does not create accounts, sign, or submit a transaction.")
                    }
                }

                Rectangle {
                    Layout.alignment: Qt.AlignTop
                    Layout.preferredHeight: 28
                    Layout.preferredWidth: prototypeLabel.implicitWidth + 18
                    radius: 14
                    color: "#211914"
                    border.color: "#49301F"
                    border.width: 1

                    Text {
                        id: prototypeLabel

                        anchors.centerIn: parent
                        color: "#F2D8C7"
                        font.pixelSize: 12
                        font.weight: Font.DemiBold
                        text: qsTr("Prototype")
                    }
                }
            }

            GridLayout {
                id: workbench

                readonly property int columnCount: content.width >= 1220 ? 3 : content.width >= 820 ? 2 : 1

                Layout.fillWidth: true
                columns: columnCount
                columnSpacing: 14
                rowSpacing: 14

                Rectangle {
                    id: formPanel

                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    Layout.preferredWidth: workbench.columnCount === 3 ? 520 : 0
                    Layout.columnSpan: workbench.columnCount === 1 ? 1 : 1
                    implicitHeight: formContent.implicitHeight + 32
                    radius: 16
                    color: "#1B1B1B"
                    border.color: "#303030"
                    border.width: 1

                    ColumnLayout {
                        id: formContent

                        anchors.fill: parent
                        anchors.margins: 16
                        spacing: 14

                        Text {
                            color: "#E7E1D8"
                            font.pixelSize: 18
                            font.weight: Font.DemiBold
                            text: qsTr("Definition settings")
                        }

                        TabBar {
                            id: kindTabs

                            Layout.fillWidth: true
                            Layout.preferredHeight: 42
                            currentIndex: root.tokenKind

                            background: Rectangle {
                                radius: 8
                                color: "#101010"
                                border.color: "#343434"
                                border.width: 1
                            }

                            onCurrentIndexChanged: {
                                root.tokenKind = currentIndex;
                                if (root.requiresMetadata)
                                    root.metadataEnabled = true;
                                root.prepared = false;
                            }

                            TabButton {
                                id: fungibleTab

                                text: qsTr("Fungible")
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Create fungible token definition")

                                contentItem: Text {
                                    color: fungibleTab.checked ? "#F2D8C7" : "#A9A098"
                                    font.pixelSize: 14
                                    font.weight: fungibleTab.checked ? Font.DemiBold : Font.Normal
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: fungibleTab.text
                                }

                                background: Rectangle {
                                    radius: 7
                                    color: fungibleTab.checked ? "#211914" : "transparent"
                                    border.color: fungibleTab.activeFocus ? "#FFB26B" : fungibleTab.checked ? "#F26A21" : "transparent"
                                    border.width: fungibleTab.activeFocus ? 2 : 1
                                }
                            }

                            TabButton {
                                id: nonFungibleTab

                                text: qsTr("Non-fungible")
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Create non-fungible token definition")

                                contentItem: Text {
                                    color: nonFungibleTab.checked ? "#F2D8C7" : "#A9A098"
                                    font.pixelSize: 14
                                    font.weight: nonFungibleTab.checked ? Font.DemiBold : Font.Normal
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: nonFungibleTab.text
                                }

                                background: Rectangle {
                                    radius: 7
                                    color: nonFungibleTab.checked ? "#211914" : "transparent"
                                    border.color: nonFungibleTab.activeFocus ? "#FFB26B" : nonFungibleTab.checked ? "#F26A21" : "transparent"
                                    border.width: nonFungibleTab.activeFocus ? 2 : 1
                                }
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            color: "#A9A098"
                            font.pixelSize: 13
                            wrapMode: Text.Wrap
                            text: root.isFungible ? qsTr("Fungible definitions create one initial holding with the full raw supply.") : qsTr("NFT definitions always include metadata and create a master holding that controls printing.")
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: "#303030"
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 7

                            Text {
                                color: "#E7E1D8"
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                text: qsTr("Identity and supply")
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: qsTr("Name")
                            }

                            TextField {
                                id: nameField

                                Layout.fillWidth: true
                                Layout.preferredHeight: 42
                                Accessible.name: qsTr("Token definition name")
                                color: "#E7E1D8"
                                font.pixelSize: 15
                                placeholderText: qsTr("Name stored on the definition")
                                placeholderTextColor: "#8E8780"
                                selectByMouse: true
                                text: qsTr("New token")

                                onTextEdited: root.prepared = false

                                background: Rectangle {
                                    radius: 6
                                    color: "#101010"
                                    border.color: nameField.activeFocus ? "#F26A21" : "#343434"
                                    border.width: 1
                                }
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: root.supplyLabel
                            }

                            TextField {
                                id: supplyField

                                Layout.fillWidth: true
                                Layout.preferredHeight: 42
                                Accessible.name: root.supplyLabel
                                color: "#E7E1D8"
                                font.pixelSize: 15
                                inputMethodHints: Qt.ImhDigitsOnly
                                placeholderText: qsTr("Unsigned 128-bit integer")
                                placeholderTextColor: "#8E8780"
                                selectByMouse: true
                                text: "1000000"
                                validator: RegularExpressionValidator {
                                    regularExpression: /^[0-9]*$/
                                }

                                onTextEdited: root.prepared = false

                                background: Rectangle {
                                    radius: 6
                                    color: "#101010"
                                    border.color: supplyField.activeFocus ? "#F26A21" : supplyField.text.length > 0 && !root.validSupply ? "#D85F4B" : "#343434"
                                    border.width: 1
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                color: root.validSupply ? "#A9A098" : "#F08A76"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: root.validSupply ? qsTr("Raw u128 value. Decimal display precision is not stored by the Token Program.") : qsTr("Enter an unsigned 128-bit integer (0 through 340282366920938463463374607431768211455).")
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            color: "#303030"
                            visible: root.isFungible
                            Layout.preferredHeight: visible ? 1 : 0
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: root.isFungible
                            Layout.preferredHeight: visible ? implicitHeight : 0
                            spacing: 7

                            Text {
                                color: "#E7E1D8"
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                text: qsTr("Mint authority")
                            }

                            Text {
                                Layout.fillWidth: true
                                color: "#A9A098"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: qsTr("This is the full authority surface at creation. NFTs have no definition-level mint authority.")
                            }

                            RadioButton {
                                id: fixedAuthority

                                Layout.fillWidth: true
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Fixed supply with no mint authority")
                                checked: root.authorityMode === 0
                                text: qsTr("Fixed supply — no future minting")
                                onClicked: {
                                    root.authorityMode = 0;
                                    root.prepared = false;
                                }

                                contentItem: Text {
                                    leftPadding: fixedAuthority.indicator.width + fixedAuthority.spacing
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    text: fixedAuthority.text
                                    verticalAlignment: Text.AlignVCenter
                                    wrapMode: Text.Wrap
                                }

                                indicator: Rectangle {
                                    implicitWidth: 18
                                    implicitHeight: 18
                                    x: fixedAuthority.leftPadding
                                    y: fixedAuthority.topPadding + (fixedAuthority.availableHeight - height) / 2
                                    radius: 9
                                    color: "#101010"
                                    border.color: fixedAuthority.activeFocus ? "#FFB26B" : fixedAuthority.checked ? "#F26A21" : "#8E8780"
                                    border.width: fixedAuthority.activeFocus ? 2 : 1

                                    Rectangle {
                                        anchors.centerIn: parent
                                        width: 8
                                        height: 8
                                        radius: 4
                                        color: "#F26A21"
                                        visible: fixedAuthority.checked
                                    }
                                }
                            }

                            RadioButton {
                                id: selfAuthority

                                Layout.fillWidth: true
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Use definition account as mint authority")
                                checked: root.authorityMode === 1
                                text: qsTr("Self authority — definition account signs future mints")
                                onClicked: {
                                    root.authorityMode = 1;
                                    root.prepared = false;
                                }

                                contentItem: Text {
                                    leftPadding: selfAuthority.indicator.width + selfAuthority.spacing
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    text: selfAuthority.text
                                    verticalAlignment: Text.AlignVCenter
                                    wrapMode: Text.Wrap
                                }

                                indicator: Rectangle {
                                    implicitWidth: 18
                                    implicitHeight: 18
                                    x: selfAuthority.leftPadding
                                    y: selfAuthority.topPadding + (selfAuthority.availableHeight - height) / 2
                                    radius: 9
                                    color: "#101010"
                                    border.color: selfAuthority.activeFocus ? "#FFB26B" : selfAuthority.checked ? "#F26A21" : "#8E8780"
                                    border.width: selfAuthority.activeFocus ? 2 : 1

                                    Rectangle {
                                        anchors.centerIn: parent
                                        width: 8
                                        height: 8
                                        radius: 4
                                        color: "#F26A21"
                                        visible: selfAuthority.checked
                                    }
                                }
                            }

                            RadioButton {
                                id: externalAuthority

                                Layout.fillWidth: true
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Use external mint authority")
                                checked: root.authorityMode === 2
                                text: qsTr("External authority — a separate account signs future mints")
                                onClicked: {
                                    root.authorityMode = 2;
                                    root.prepared = false;
                                }

                                contentItem: Text {
                                    leftPadding: externalAuthority.indicator.width + externalAuthority.spacing
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    text: externalAuthority.text
                                    verticalAlignment: Text.AlignVCenter
                                    wrapMode: Text.Wrap
                                }

                                indicator: Rectangle {
                                    implicitWidth: 18
                                    implicitHeight: 18
                                    x: externalAuthority.leftPadding
                                    y: externalAuthority.topPadding + (externalAuthority.availableHeight - height) / 2
                                    radius: 9
                                    color: "#101010"
                                    border.color: externalAuthority.activeFocus ? "#FFB26B" : externalAuthority.checked ? "#F26A21" : "#8E8780"
                                    border.width: externalAuthority.activeFocus ? 2 : 1

                                    Rectangle {
                                        anchors.centerIn: parent
                                        width: 8
                                        height: 8
                                        radius: 4
                                        color: "#F26A21"
                                        visible: externalAuthority.checked
                                    }
                                }
                            }

                            TextField {
                                id: externalAuthorityField

                                Layout.fillWidth: true
                                Layout.preferredHeight: root.authorityMode === 2 ? 42 : 0
                                visible: root.authorityMode === 2
                                Accessible.name: qsTr("External mint authority account ID")
                                clip: true
                                color: "#E7E1D8"
                                font.pixelSize: 14
                                placeholderText: qsTr("External authority account ID")
                                placeholderTextColor: "#8E8780"
                                selectByMouse: true

                                onTextEdited: root.prepared = false

                                background: Rectangle {
                                    radius: 6
                                    color: "#101010"
                                    border.color: externalAuthorityField.activeFocus ? "#F26A21" : externalAuthorityField.text.length > 0 && !root.validExternalAuthority ? "#D85F4B" : "#343434"
                                    border.width: 1
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: "#303030"
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 8

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 10

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 3

                                    Text {
                                        color: "#E7E1D8"
                                        font.pixelSize: 15
                                        font.weight: Font.DemiBold
                                        text: qsTr("Metadata")
                                    }

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        text: root.requiresMetadata ? qsTr("Required for non-fungible definitions") : qsTr("Optional for fungible definitions")
                                    }
                                }

                                Switch {
                                    id: metadataSwitch

                                    Layout.alignment: Qt.AlignVCenter
                                    Accessible.name: qsTr("Include metadata account")
                                    checked: root.hasMetadata
                                    enabled: !root.requiresMetadata
                                    onToggled: {
                                        root.metadataEnabled = checked;
                                        root.prepared = false;
                                    }
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                color: "#A9A098"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: qsTr("The program stores only standard, URI, creators, definition link, and a primary-sale date initialized to 0. It does not validate URI content or metadata schema.")
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.hasMetadata
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 7

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        text: qsTr("Metadata standard")
                                    }

                                    Button {
                                        id: simpleMetadataButton

                                        Layout.preferredHeight: 30
                                        text: qsTr("Simple")
                                        checkable: true
                                        checked: root.metadataStandard === 0
                                        activeFocusOnTab: true
                                        Accessible.name: qsTr("Use Simple metadata standard")
                                        onClicked: {
                                            root.metadataStandard = 0;
                                            root.prepared = false;
                                        }

                                        contentItem: Text {
                                            color: simpleMetadataButton.checked ? "#F2D8C7" : "#A9A098"
                                            font.pixelSize: 12
                                            horizontalAlignment: Text.AlignHCenter
                                            verticalAlignment: Text.AlignVCenter
                                            text: simpleMetadataButton.text
                                        }

                                        background: Rectangle {
                                            radius: 6
                                            color: simpleMetadataButton.checked ? "#211914" : "#101010"
                                            border.color: simpleMetadataButton.activeFocus ? "#FFB26B" : simpleMetadataButton.checked ? "#F26A21" : "#343434"
                                            border.width: simpleMetadataButton.activeFocus ? 2 : 1
                                        }
                                    }

                                    Button {
                                        id: expandedMetadataButton

                                        Layout.preferredHeight: 30
                                        text: qsTr("Expanded")
                                        checkable: true
                                        checked: root.metadataStandard === 1
                                        activeFocusOnTab: true
                                        Accessible.name: qsTr("Use Expanded metadata standard")
                                        onClicked: {
                                            root.metadataStandard = 1;
                                            root.prepared = false;
                                        }

                                        contentItem: Text {
                                            color: expandedMetadataButton.checked ? "#F2D8C7" : "#A9A098"
                                            font.pixelSize: 12
                                            horizontalAlignment: Text.AlignHCenter
                                            verticalAlignment: Text.AlignVCenter
                                            text: expandedMetadataButton.text
                                        }

                                        background: Rectangle {
                                            radius: 6
                                            color: expandedMetadataButton.checked ? "#211914" : "#101010"
                                            border.color: expandedMetadataButton.activeFocus ? "#FFB26B" : expandedMetadataButton.checked ? "#F26A21" : "#343434"
                                            border.width: expandedMetadataButton.activeFocus ? 2 : 1
                                        }
                                    }

                                    Item {
                                        Layout.fillWidth: true
                                    }
                                }

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("URI")
                                }

                                TextField {
                                    id: metadataUriField

                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 42
                                    Accessible.name: qsTr("Metadata URI")
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    placeholderText: qsTr("data:application/json;base64,... or external URI")
                                    placeholderTextColor: "#8E8780"
                                    selectByMouse: true

                                    onTextEdited: root.prepared = false

                                    background: Rectangle {
                                        radius: 6
                                        color: "#101010"
                                        border.color: metadataUriField.activeFocus ? "#F26A21" : "#343434"
                                        border.width: 1
                                    }
                                }

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Creators")
                                }

                                TextField {
                                    id: creatorsField

                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 42
                                    Accessible.name: qsTr("Metadata creators string")
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    placeholderText: qsTr("Creator account or attribution string")
                                    placeholderTextColor: "#8E8780"
                                    selectByMouse: true

                                    onTextEdited: root.prepared = false

                                    background: Rectangle {
                                        radius: 6
                                        color: "#101010"
                                        border.color: creatorsField.activeFocus ? "#F26A21" : "#343434"
                                        border.width: 1
                                    }
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: "#303030"
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 7

                            Text {
                                color: "#E7E1D8"
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                text: qsTr("New target accounts")
                            }

                            Text {
                                Layout.fillWidth: true
                                color: "#A9A098"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: qsTr("Every target must be a new default-valued account and authorize this transaction. The prototype can validate shape, not on-chain account state.")
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: qsTr("Definition target account")
                            }

                            TextField {
                                id: definitionTargetField

                                Layout.fillWidth: true
                                Layout.preferredHeight: 42
                                Accessible.name: qsTr("Definition target account ID")
                                clip: true
                                color: "#E7E1D8"
                                font.pixelSize: 14
                                placeholderText: qsTr("New authorized account ID")
                                placeholderTextColor: "#8E8780"
                                selectByMouse: true

                                onTextEdited: root.prepared = false

                                background: Rectangle {
                                    radius: 6
                                    color: "#101010"
                                    border.color: definitionTargetField.activeFocus ? "#F26A21" : definitionTargetField.text.length > 0 && !root.validDefinitionTarget ? "#D85F4B" : "#343434"
                                    border.width: 1
                                }
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: root.isFungible ? qsTr("Initial fungible holding target") : qsTr("NFT master holding target")
                            }

                            TextField {
                                id: holdingTargetField

                                Layout.fillWidth: true
                                Layout.preferredHeight: 42
                                Accessible.name: root.isFungible ? qsTr("Initial fungible holding target account ID") : qsTr("NFT master holding target account ID")
                                clip: true
                                color: "#E7E1D8"
                                font.pixelSize: 14
                                placeholderText: qsTr("New authorized account ID")
                                placeholderTextColor: "#8E8780"
                                selectByMouse: true

                                onTextEdited: root.prepared = false

                                background: Rectangle {
                                    radius: 6
                                    color: "#101010"
                                    border.color: holdingTargetField.activeFocus ? "#F26A21" : holdingTargetField.text.length > 0 && !root.validHoldingTarget ? "#D85F4B" : "#343434"
                                    border.width: 1
                                }
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.hasMetadata
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 7

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Metadata target account")
                                }

                                TextField {
                                    id: metadataTargetField

                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 42
                                    Accessible.name: qsTr("Metadata target account ID")
                                    clip: true
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    placeholderText: qsTr("New authorized account ID")
                                    placeholderTextColor: "#8E8780"
                                    selectByMouse: true

                                    onTextEdited: root.prepared = false

                                    background: Rectangle {
                                        radius: 6
                                        color: "#101010"
                                        border.color: metadataTargetField.activeFocus ? "#F26A21" : metadataTargetField.text.length > 0 && !root.validMetadataTarget ? "#D85F4B" : "#343434"
                                        border.width: 1
                                    }
                                }
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 8

                            Button {
                                id: fixedExampleButton

                                Layout.preferredHeight: 34
                                text: qsTr("Fixed example")
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Load fixed supply creation example")
                                onClicked: root.setPattern("fixed")

                                contentItem: Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: fixedExampleButton.text
                                }

                                background: Rectangle {
                                    radius: 6
                                    color: "#101010"
                                    border.color: fixedExampleButton.activeFocus ? "#FFB26B" : "#343434"
                                    border.width: fixedExampleButton.activeFocus ? 2 : 1
                                }
                            }

                            Button {
                                id: metadataExampleButton

                                Layout.preferredHeight: 34
                                text: qsTr("Metadata example")
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Load metadata token creation example")
                                onClicked: root.setPattern("metadata")

                                contentItem: Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: metadataExampleButton.text
                                }

                                background: Rectangle {
                                    radius: 6
                                    color: "#101010"
                                    border.color: metadataExampleButton.activeFocus ? "#FFB26B" : "#343434"
                                    border.width: metadataExampleButton.activeFocus ? 2 : 1
                                }
                            }

                            Button {
                                id: nftExampleButton

                                Layout.preferredHeight: 34
                                text: qsTr("NFT example")
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Load non-fungible collection creation example")
                                onClicked: root.setPattern("nft")

                                contentItem: Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: nftExampleButton.text
                                }

                                background: Rectangle {
                                    radius: 6
                                    color: "#101010"
                                    border.color: nftExampleButton.activeFocus ? "#FFB26B" : "#343434"
                                    border.width: nftExampleButton.activeFocus ? 2 : 1
                                }
                            }

                            Item {
                                Layout.fillWidth: true
                            }
                        }
                    }
                }

                Rectangle {
                    id: previewPanel

                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    Layout.preferredWidth: workbench.columnCount === 3 ? 410 : 0
                    Layout.columnSpan: workbench.columnCount === 1 ? 1 : 1
                    implicitHeight: previewContent.implicitHeight + 32
                    radius: 16
                    color: "#1B1B1B"
                    border.color: "#303030"
                    border.width: 1

                    ColumnLayout {
                        id: previewContent

                        anchors.fill: parent
                        anchors.margins: 16
                        spacing: 13

                        Text {
                            color: "#E7E1D8"
                            font.pixelSize: 18
                            font.weight: Font.DemiBold
                            text: qsTr("Definition preview")
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: previewHeadline.implicitHeight + 22
                            radius: 10
                            color: root.isFungible ? "#211914" : "#181D25"
                            border.color: root.isFungible ? "#49301F" : "#31435D"
                            border.width: 1

                            RowLayout {
                                id: previewHeadline

                                anchors.fill: parent
                                anchors.margins: 11
                                spacing: 10

                                Rectangle {
                                    Layout.preferredHeight: 30
                                    Layout.preferredWidth: kindBadge.implicitWidth + 16
                                    radius: 15
                                    color: root.isFungible ? "#2D211A" : "#182534"
                                    border.color: root.isFungible ? "#6A4329" : "#40607A"
                                    border.width: 1

                                    Text {
                                        id: kindBadge

                                        anchors.centerIn: parent
                                        color: root.isFungible ? "#F2D8C7" : "#BFD8F4"
                                        font.pixelSize: 12
                                        font.weight: Font.DemiBold
                                        text: root.isFungible ? qsTr("Fungible") : qsTr("NFT collection")
                                    }
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    elide: Text.ElideRight
                                    font.pixelSize: 16
                                    font.weight: Font.DemiBold
                                    text: nameField.text.length > 0 ? nameField.text : qsTr("Untitled definition")
                                }
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 9

                            RowLayout {
                                Layout.fillWidth: true

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: root.isFungible ? qsTr("Stored total supply") : qsTr("Stored printable supply")
                                }

                                Item {
                                    Layout.fillWidth: true
                                }

                                Text {
                                    color: root.validSupply ? "#E7E1D8" : "#F08A76"
                                    font.pixelSize: 14
                                    font.weight: Font.Medium
                                    text: root.supplyValue
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 1
                                color: "#303030"
                            }

                            RowLayout {
                                Layout.fillWidth: true

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Creation instruction")
                                }

                                Item {
                                    Layout.fillWidth: true
                                }

                                Text {
                                    color: "#F2D8C7"
                                    font.family: "monospace"
                                    font.pixelSize: 12
                                    text: root.instructionName
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 1
                                color: "#303030"
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.isFungible
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 5

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Mint authority")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    wrapMode: Text.Wrap
                                    text: root.authorityValue
                                }
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: !root.isFungible
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 5

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Master holding behavior")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    wrapMode: Text.Wrap
                                    text: qsTr("Starts with print balance %1. The master remains reserved, so at most %2 printed copies are possible.").arg(root.supplyValue).arg(root.validSupply && supplyField.text !== "0" ? qsTr("one fewer than the printable supply") : qsTr("none until a positive printable supply is set"))
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: root.hasMetadata ? 1 : 0
                                visible: root.hasMetadata
                                color: "#303030"
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.hasMetadata
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 6

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Metadata account")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    elide: Text.ElideMiddle
                                    font.family: "monospace"
                                    font.pixelSize: 12
                                    text: metadataTargetField.text.length > 0 ? metadataTargetField.text : qsTr("New metadata target required")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    wrapMode: Text.Wrap
                                    text: qsTr("%1 · URI and creators are stored as supplied.").arg(root.metadataStandard === 0 ? qsTr("Simple") : qsTr("Expanded"))
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: holdingOutcome.implicitHeight + 24
                            radius: 10
                            color: "#101010"
                            border.color: "#343434"
                            border.width: 1

                            ColumnLayout {
                                id: holdingOutcome

                                anchors.fill: parent
                                anchors.margins: 12
                                spacing: 6

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: root.isFungible ? qsTr("Initial fungible holding") : qsTr("Initial NFT master holding")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    elide: Text.ElideMiddle
                                    font.family: "monospace"
                                    font.pixelSize: 12
                                    text: holdingTargetField.text.length > 0 ? holdingTargetField.text : qsTr("New holding target required")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    wrapMode: Text.Wrap
                                    text: root.isFungible ? qsTr("Receives the full initial raw supply: %1.").arg(root.supplyValue) : qsTr("Receives the master state and print balance: %1.").arg(root.supplyValue)
                                }
                            }
                        }

                        Button {
                            id: prepareButton

                            Layout.fillWidth: true
                            Layout.preferredHeight: 46
                            activeFocusOnTab: true
                            Accessible.name: qsTr("Prepare token definition draft")
                            enabled: root.canPrepare
                            text: root.canPrepare ? qsTr("Prepare definition") : qsTr("Complete required targets")
                            onClicked: root.prepareDefinition()

                            contentItem: Text {
                                color: prepareButton.enabled ? "#FFFFFF" : "#8E8780"
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                                text: prepareButton.text
                            }

                            background: Rectangle {
                                radius: 8
                                color: prepareButton.enabled ? "#F26A21" : "#282522"
                                border.color: prepareButton.activeFocus ? "#FFB26B" : prepareButton.enabled ? "#F26A21" : "#3C3833"
                                border.width: prepareButton.activeFocus ? 2 : 1
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            visible: root.prepared
                            Layout.preferredHeight: visible ? implicitHeight : 0
                            color: "#78C88D"
                            font.pixelSize: 12
                            wrapMode: Text.Wrap
                            text: root.preparedMessage
                        }
                    }
                }

                Rectangle {
                    id: readinessPanel

                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    Layout.preferredWidth: workbench.columnCount === 3 ? 300 : 0
                    Layout.columnSpan: workbench.columnCount === 1 ? 1 : workbench.columnCount === 2 ? 2 : 1
                    implicitHeight: readinessContent.implicitHeight + 32
                    radius: 16
                    color: "#181818"
                    border.color: "#303030"
                    border.width: 1

                    ColumnLayout {
                        id: readinessContent

                        anchors.fill: parent
                        anchors.margins: 16
                        spacing: 12

                        Text {
                            color: "#E7E1D8"
                            font.pixelSize: 18
                            font.weight: Font.DemiBold
                            text: qsTr("Protocol readiness")
                        }

                        Text {
                            Layout.fillWidth: true
                            color: "#A9A098"
                            font.pixelSize: 13
                            wrapMode: Text.Wrap
                            text: qsTr("Creation requires fresh authorized target accounts. Green checks confirm form shape only; account state remains unverified until a real client reads the chain.")
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: "#303030"
                        }

                        Repeater {
                            model: [
                                {
                                    label: qsTr("Definition target"),
                                    detail: qsTr("init · signer · writable"),
                                    ok: root.validDefinitionTarget
                                },
                                {
                                    label: root.isFungible ? qsTr("Initial holding") : qsTr("NFT master holding"),
                                    detail: qsTr("init · signer · writable"),
                                    ok: root.validHoldingTarget
                                },
                                {
                                    label: qsTr("Metadata target"),
                                    detail: root.hasMetadata ? qsTr("init · signer · writable") : qsTr("not used by this instruction"),
                                    ok: root.hasMetadata ? root.validMetadataTarget : true
                                }
                            ]

                            delegate: RowLayout {
                                id: readinessRow

                                required property var modelData

                                Layout.fillWidth: true
                                spacing: 9

                                Rectangle {
                                    Layout.preferredHeight: 24
                                    Layout.preferredWidth: readinessState.implicitWidth + 14
                                    radius: 12
                                    color: readinessRow.modelData.ok ? "#183222" : "#332521"
                                    border.color: readinessRow.modelData.ok ? "#39C06A" : "#D85F4B"
                                    border.width: 1

                                    Text {
                                        id: readinessState

                                        anchors.centerIn: parent
                                        color: readinessRow.modelData.ok ? "#78C88D" : "#F08A76"
                                        font.pixelSize: 11
                                        font.weight: Font.DemiBold
                                        text: readinessRow.modelData.ok ? qsTr("Ready") : qsTr("Needs input")
                                    }
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Text {
                                        color: "#E7E1D8"
                                        font.pixelSize: 14
                                        font.weight: Font.Medium
                                        text: readinessRow.modelData.label
                                    }

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        text: readinessRow.modelData.detail
                                    }
                                }
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: root.isFungible && root.authorityMode === 2
                            Layout.preferredHeight: visible ? implicitHeight : 0
                            spacing: 7

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 1
                                color: "#303030"
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 9

                                Rectangle {
                                    Layout.preferredHeight: 24
                                    Layout.preferredWidth: externalAuthorityState.implicitWidth + 14
                                    radius: 12
                                    color: root.validExternalAuthority ? "#183222" : "#332521"
                                    border.color: root.validExternalAuthority ? "#39C06A" : "#D85F4B"
                                    border.width: 1

                                    Text {
                                        id: externalAuthorityState

                                        anchors.centerIn: parent
                                        color: root.validExternalAuthority ? "#78C88D" : "#F08A76"
                                        font.pixelSize: 11
                                        font.weight: Font.DemiBold
                                        text: root.validExternalAuthority ? qsTr("Ready") : qsTr("Needs input")
                                    }
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Text {
                                        color: "#E7E1D8"
                                        font.pixelSize: 14
                                        font.weight: Font.Medium
                                        text: qsTr("External mint authority")
                                    }

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        text: qsTr("Must be a non-zero account ID")
                                    }
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: unsupportedNotice.implicitHeight + 22
                            radius: 8
                            color: "#211914"
                            border.color: "#49301F"
                            border.width: 1

                            Text {
                                id: unsupportedNotice

                                anchors.fill: parent
                                anchors.margins: 11
                                color: "#F2D8C7"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: root.hasMetadata ? qsTr("Metadata-backed definitions require typed serialization today; the generic IDL route cannot serialize the structured creation arguments.") : qsTr("This simple fungible path maps to the generic creation instruction, but this prototype never submits it.")
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            color: "#8E8780"
                            font.pixelSize: 12
                            wrapMode: Text.Wrap
                            text: qsTr("No on-chain symbol, decimal field, image, royalty, collection, or mutable metadata setting exists in the current token definition schema.")
                        }
                    }
                }
            }
        }
    }
}
