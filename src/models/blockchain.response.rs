enum BlockchainOperation {
    Transaction,
    Block,
}

// struct Transaction 

pub struct BlockchainResponse {
  pub operation: BlockchainOperation, 
}