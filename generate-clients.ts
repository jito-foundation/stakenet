import { createFromRoot } from "codama";
import { rootNodeFromAnchor } from "@codama/nodes-from-anchor";
import { renderJavaScriptVisitor } from "@codama/renderers";
import path from "path";
import { promises as fs } from "fs";

// Find the Anchor IDL file and return the JSON object
const loadAnchorIDL = async (program: string) => {
  const basePath = path.join("programs", program, "idl");
  const dirPath = path.join(basePath);

  try {
    // Read the directory contents
    const files = await fs.readdir(dirPath);
    const jsonFiles = files.filter((file) => file.endsWith(".json"));

    if (!jsonFiles.length) {
      throw new Error(`No JSON files found in ${dirPath}`);
    }

    if (jsonFiles.length > 1) {
      throw new Error(
        `Multiple JSON files found in ${dirPath}. Please specify which one to use.`
      );
    }

    const filePath = path.join(dirPath, jsonFiles[0]);
    return JSON.parse(await fs.readFile(filePath, "utf-8"));
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") {
      throw new Error(`Failed to load IDL: ${dirPath} does not exist`);
    }
    throw error;
  }
};

// Generate Steward Client
const stewardIdl = await loadAnchorIDL("steward");
const stewardCodama = createFromRoot(rootNodeFromAnchor(stewardIdl));
const generatedStewardPath = path.join("packages", "steward-sdk", "src");
stewardCodama.accept(renderJavaScriptVisitor(generatedStewardPath));

// Generate Validator History Client
const validatorHistoryIdl = await loadAnchorIDL("validator-history");
const validatorHistoryCodama = createFromRoot(rootNodeFromAnchor(validatorHistoryIdl));
const generatedValidatorHistoryPath = path.join("packages", "validator-history-sdk", "src");
validatorHistoryCodama.accept(renderJavaScriptVisitor(generatedValidatorHistoryPath));