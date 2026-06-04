use std::path::PathBuf;

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;

use crate::models::OrgNode;
use crate::storage;

fn generate_id() -> String {
    format!("node_{}_{}", Utc::now().timestamp_millis(), &random_str(6))
}

fn find_node<'a>(tree: &'a mut OrgNode, id: &str) -> Option<&'a mut OrgNode> {
    if tree.id == id {
        return Some(tree);
    }
    for child in &mut tree.children {
        if let Some(found) = find_node(child, id) {
            return Some(found);
        }
    }
    None
}

fn remove_child_from_tree(tree: &mut OrgNode, id: &str) -> bool {
    if let Some(pos) = tree.children.iter().position(|c| c.id == id) {
        tree.children.remove(pos);
        return true;
    }
    for child in &mut tree.children {
        if remove_child_from_tree(child, id) {
            return true;
        }
    }
    false
}

fn collect_members_from(node: &OrgNode) -> Vec<String> {
    let mut members = node.members.clone();
    for child in &node.children {
        members.extend(collect_members_from(child));
    }
    members.sort();
    members.dedup();
    members
}

pub async fn get_tree(
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let tree: OrgNode = storage::JsonStore::new(base.join("org_tree.json")).read();
    Ok(serde_json::json!({
        "success": true,
        "tree": tree
    }).into())
}

pub async fn add_node(
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let parent_id = body.get("parentId").and_then(|v| v.as_str()).unwrap_or("");
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let node_type = body.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if name.is_empty() || node_type.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "名称和类型不能为空"
        }).into());
    }

    let mut tree: OrgNode = storage::JsonStore::new(base.join("org_tree.json")).read();
    let parent = find_node(&mut tree, parent_id);

    match parent {
        Some(p) => {
            let new_node = OrgNode {
                id: generate_id(),
                name: name.trim().to_string(),
                node_type: node_type.to_string(),
                children: vec![],
                members: vec![],
            };
            p.children.push(new_node.clone());
            storage::JsonStore::new(base.join("org_tree.json")).write(&tree);
            Ok(serde_json::json!({
                "success": true,
                "node": new_node,
                "message": "节点添加成功"
            }).into())
        }
        None => Ok(serde_json::json!({
            "success": false, "message": "父节点不存在"
        }).into()),
    }
}

pub async fn delete_node(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut tree: OrgNode = storage::JsonStore::new(base.join("org_tree.json")).read();

    if id == tree.id {
        return Ok(serde_json::json!({
            "success": false, "message": "不能删除根节点"
        }).into());
    }

    if remove_child_from_tree(&mut tree, &id) {
        storage::JsonStore::new(base.join("org_tree.json")).write(&tree);
        Ok(serde_json::json!({
            "success": true, "message": "节点已删除"
        }).into())
    } else {
        Ok(serde_json::json!({
            "success": false, "message": "节点不存在"
        }).into())
    }
}

pub async fn update_members(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(base): State<PathBuf>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let user_ids: Vec<String> = body.get("userIds")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    if action.is_empty() || user_ids.is_empty() {
        return Ok(serde_json::json!({
            "success": false, "message": "参数无效"
        }).into());
    }

    let mut tree: OrgNode = storage::JsonStore::new(base.join("org_tree.json")).read();

    // Use recursive approach without holding a mutable reference
    fn update_node_members(node: &mut OrgNode, id: &str, action: &str, user_ids: &[String]) -> bool {
        if node.id == id {
            match action {
                "add" => {
                    for uid in user_ids {
                        if !node.members.contains(uid) {
                            node.members.push(uid.clone());
                        }
                    }
                }
                "remove" => {
                    node.members.retain(|m| !user_ids.contains(m));
                }
                _ => {}
            }
            return true;
        }
        for child in &mut node.children {
            if update_node_members(child, id, action, user_ids) {
                return true;
            }
        }
        false
    }

    let found = update_node_members(&mut tree, &id, &action, &user_ids);

    if found {
        let members = tree_members_by_id(&tree, &id);
        storage::JsonStore::new(base.join("org_tree.json")).write(&tree);
        Ok(serde_json::json!({
            "success": true,
            "members": members,
            "message": "成员更新成功"
        }).into())
    } else {
        Ok(serde_json::json!({
            "success": false, "message": "节点不存在"
        }).into())
    }
}

pub async fn get_node_members(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let tree: OrgNode = storage::JsonStore::new(base.join("org_tree.json")).read();

    fn find_node_in_tree<'a>(node: &'a OrgNode, id: &str) -> Option<&'a OrgNode> {
        if node.id == id { return Some(node); }
        for child in &node.children {
            if let Some(found) = find_node_in_tree(child, id) {
                return Some(found);
            }
        }
        None
    }

    match find_node_in_tree(&tree, &id) {
        Some(node) => {
            let members = collect_members_from(node);
            Ok(serde_json::json!({
                "success": true,
                "members": members,
                "count": members.len()
            }).into())
        }
        None => Ok(serde_json::json!({
            "success": false, "message": "节点不存在"
        }).into()),
    }
}

pub async fn get_flat_nodes(
    State(base): State<PathBuf>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let tree: OrgNode = storage::JsonStore::new(base.join("org_tree.json")).read();
    let mut nodes: Vec<serde_json::Value> = vec![];

    fn walk(node: &OrgNode, depth: usize, nodes: &mut Vec<serde_json::Value>) {
        nodes.push(serde_json::json!({
            "id": node.id,
            "name": node.name,
            "type": node.node_type,
            "depth": depth,
            "memberCount": node.members.len()
        }));
        for child in &node.children {
            walk(child, depth + 1, nodes);
        }
    }

    walk(&tree, 0, &mut nodes);

    Ok(serde_json::json!({
        "success": true,
        "nodes": nodes
    }).into())
}

fn tree_members_by_id(tree: &OrgNode, id: &str) -> Vec<String> {
    fn find_node_ref<'a>(node: &'a OrgNode, id: &str) -> Option<&'a OrgNode> {
        if node.id == id { Some(node) }
        else {
            for child in &node.children {
                if let Some(found) = find_node_ref(child, id) {
                    return Some(found);
                }
            }
            None
        }
    }
    find_node_ref(tree, id).map(|n| n.members.clone()).unwrap_or_default()
}

fn random_str(len: usize) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();
    (0..len).map(|i| {
        let idx = (seed.wrapping_mul(i as u64 + 1).wrapping_add(seed >> (i % 8))) as usize % chars.len();
        chars[idx]
    }).collect()
}
